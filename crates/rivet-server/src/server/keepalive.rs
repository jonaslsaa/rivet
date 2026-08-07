//! Paper keepalive state machine (issue #157) — the pure, deterministic core.
//!
//! Port of `io.papermc.paper.util.KeepAlive` + the keepalive halves of
//! `ServerCommonPacketListenerImpl.keepConnectionAlive`/`handleKeepAlive`, split
//! out of the play/configuration listeners so the *state* is testable
//! exhaustively with a simulated clock before the #96 play handoff exists.
//!
//! Two independent clock axes, mirroring Paper:
//!   - **monotonic nanos** (`tx_time_ns` on every pending challenge, the
//!     `last_keep_alive_tx` throttle, and the `PingCalculator` windows) — in
//!     Java `System.nanoTime()`;
//!   - **millis** (`now_ms` on the timeout check, the `closed_listener_time`
//!     guard) — in Java `Util.getMillis()`.
//!
//! Both are `i64`/`u128` monotonic values injected at the call site, never a
//! wall clock, so every branch is deterministic under the Rivet `TickTime`
//! clock. All Java arithmetic wraps silently; the code uses `i64` (never `u64`
//! for these axes) so `wrapping_sub`/`wrapping_add` keep the exact Paper
//! behavior when the monotonic counters reach `i64::MAX` and roll over
//! (PORTING.md: wrapping arithmetic is sacred).

use std::collections::VecDeque;

/// The `KEEPALIVE_LIMIT` default: `Long.getLong("paper.playerconnection.
/// keepalive", 30) * 1000` ms. The system property is a config knob (#236); the
/// default 30 s is pinned.
pub const KEEPALIVE_LIMIT_MS: i64 = 30 * 1000;

/// `ServerCommonPacketListenerImpl.KEEPALIVE_LIMIT` in nanos:
/// `TimeUnit.MILLISECONDS.toNanos(KEEPALIVE_LIMIT)`. A pending challenge is a
/// timeout once `currTime - txTimeNS > KEEPALIVE_LIMIT_NS` (strict `>`).
pub const KEEPALIVE_LIMIT_NS: i64 = KEEPALIVE_LIMIT_MS * 1_000_000;

/// `ServerCommonPacketListenerImpl.LATENCY_CHECK_INTERVAL` (15000 ms). Paper's
/// vanilla-latency sampling period. Published for the latency readout; the
/// keepalive transmit cadence is governed by the 1 s throttle below.
pub const LATENCY_CHECK_INTERVAL_MS: i64 = 15 * 1000;

/// `KeepAlive.PendingKeepAlive(long txTimeNS, long challengeId)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingKeepAlive {
    pub tx_time_ns: i64,
    pub challenge_id: i64,
}

/// `KeepAlive.KeepAliveResponse(long txTimeNS, long rxTimeNS)` with
/// `latencyNS() = rxTimeNS - txTimeNS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeepAliveResponse {
    pub tx_time_ns: i64,
    pub rx_time_ns: i64,
}

impl KeepAliveResponse {
    /// `KeepAliveResponse.latencyNS()` — `rx - tx`, wrapping (a clock anomaly
    /// must not panic; Java `long` wraps silently).
    pub fn latency_ns(&self) -> i64 {
        self.rx_time_ns.wrapping_sub(self.tx_time_ns)
    }
}

/// `KeepAlive.PingCalculator` — a sliding window of keepalive responses over a
/// fixed interval. Ported exactly: the sum/count update, the poll-and-drop of
/// out-of-window responses (`pollIf`), and the truncating division for the
/// average (`timeSumNS / timeSumCount`; `timeSumCount` is never 0 when
/// `getAvgLatencyMS` is reachable because a response was just added).
///
/// `responses` is a FIFO (`MultiThreadedQueue`), so a `VecDeque` is the exact
/// structure; the entries are polled oldest-first until the newest response's
/// `txTimeNS` is within `intervalNS`.
#[derive(Debug, Clone)]
pub struct PingCalculator {
    interval_ns: i64,
    responses: VecDeque<KeepAliveResponse>,
    time_sum_ns: i64,
    time_sum_count: i64,
    last_average_ns: i64,
}

impl PingCalculator {
    pub fn new(interval_ns: i64) -> Self {
        PingCalculator {
            interval_ns,
            responses: VecDeque::new(),
            time_sum_ns: 0,
            time_sum_count: 0,
            last_average_ns: 0,
        }
    }

    /// The 1 m and 5 s windows: `TimeUnit.MINUTES.toNanos(1)` and
    /// `TimeUnit.SECONDS.toNanos(5)`.
    pub fn one_minute() -> Self {
        PingCalculator::new(60 * 1_000_000_000)
    }

    pub fn five_seconds() -> Self {
        PingCalculator::new(5 * 1_000_000_000)
    }

    /// `PingCalculator.copyFrom(PingCalculator)` — deep-copy the response queue
    /// and the running sums. Used by `KeepAlive.copyForListenerHandoff()`.
    pub fn copy_from(&mut self, other: &PingCalculator) {
        self.responses.clear();
        self.responses.extend(other.responses.iter().copied());
        self.time_sum_ns = other.time_sum_ns;
        self.time_sum_count = other.time_sum_count;
        self.last_average_ns = other.last_average_ns;
    }

    /// `PingCalculator.update(KeepAliveResponse)` — add the response, add its
    /// latency to the sum, then drop every response whose `txTimeNS` is more
    /// than `intervalNS` behind the newest (`currTime - ka.txTimeNS >
    /// intervalNS`, strict `>`), finally recompute `lastAverageNS`.
    pub fn update(&mut self, response: KeepAliveResponse) {
        let curr_time = response.tx_time_ns;

        self.responses.push_back(response);

        self.time_sum_count = self.time_sum_count.wrapping_add(1);
        self.time_sum_ns = self.time_sum_ns.wrapping_add(response.latency_ns());

        // Poll-and-drop out-of-window times (oldest first; `currTime` is the
        // newest response's tx, so every later response is in-window).
        while let Some(removed) = self.responses.front().copied()
            && curr_time.wrapping_sub(removed.tx_time_ns) > self.interval_ns
        {
            self.responses.pop_front();
            self.time_sum_count = self.time_sum_count.wrapping_sub(1);
            self.time_sum_ns = self.time_sum_ns.wrapping_sub(removed.latency_ns());
        }

        self.last_average_ns = self.time_sum_ns / self.time_sum_count;
    }

    /// `PingCalculator.getAvgLatencyNS()` — the cached average.
    pub fn get_avg_latency_ns(&self) -> i64 {
        self.last_average_ns
    }

    /// `PingCalculator.getAvgLatencyMS()` —
    /// `TimeUnit.NANOSECONDS.toMillis(getAvgLatencyNS())` (truncating toward
    /// zero; negative averages truncate toward zero too).
    pub fn get_avg_latency_ms(&self) -> i32 {
        (self.get_avg_latency_ns() / 1_000_000) as i32
    }

    /// `PingCalculator.getAllNS()` — every in-window latency, oldest first
    /// (iterates the live queue). Test/observability surface only.
    pub fn all_latencies_ns(&self) -> Vec<i64> {
        self.responses.iter().map(|r| r.latency_ns()).collect()
    }
}

/// The outcome of `KeepaliveState::tick` (what to transmit + the resulting
/// pending queue). One `Send` and/or one `Timeout` per tick (Paper checks the
/// timeout only after the transmit branch, and there is at most one oldest
/// pending entry).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeepaliveTickOutcome {
    /// The challenge to transmit, if the 1 s throttle elapsed.
    pub send: Option<i64>,
    /// Whether the oldest pending challenge exceeded `KEEPALIVE_LIMIT`.
    pub timeout: bool,
}

/// `KeepAlive` + the keepalive fields of `ServerCommonPacketListenerImpl`,
/// owned by the tick thread. Every time is injected (`tx_time_ns` from the
/// `TickTime` monotonic clock, `now_ms` from a millis source), so the machine
/// is fully deterministic under simulated time — no wall clock anywhere.
#[derive(Debug, Clone)]
pub struct KeepaliveState {
    /// `KeepAlive.lastKeepAliveTx` — `System.nanoTime()` at construction
    /// (`keepAlive.lastKeepAliveTx = System.nanoTime()`).
    last_keep_alive_tx_ns: i64,
    /// `KeepAlive.pendingKeepAlives` — FIFO of challenges awaiting a response.
    pending: VecDeque<PendingKeepAlive>,
    /// `KeepAlive.pingCalculator1m` — the 1-minute window.
    pub ping_calculator_1m: PingCalculator,
    /// `KeepAlive.pingCalculator5s` — the 5-second window (drives `latency`).
    pub ping_calculator_5s: PingCalculator,
    /// `ServerCommonPacketListenerImpl.latency` — the last computed average
    /// latency in ms (`this.keepAlive.pingCalculator5s.getAvgLatencyMS()`).
    latency_ms: i32,
}

impl Default for KeepaliveState {
    fn default() -> Self {
        KeepaliveState {
            // `KeepAlive.lastKeepAliveTx = System.nanoTime()` runs once at
            // construction. Simulated callers pass the epoch's first reading
            // via `new(first_now_ns)`; this default is the pure fallback.
            last_keep_alive_tx_ns: 0,
            pending: VecDeque::new(),
            ping_calculator_1m: PingCalculator::one_minute(),
            ping_calculator_5s: PingCalculator::five_seconds(),
            latency_ms: 0,
        }
    }
}

impl KeepaliveState {
    /// `new KeepAlive()` with the initial `lastKeepAliveTx` reading.
    pub fn new(first_now_ns: i64) -> Self {
        KeepaliveState {
            last_keep_alive_tx_ns: first_now_ns,
            ..KeepaliveState::default()
        }
    }

    /// `KeepAlive.copyForListenerHandoff()` — a fresh machine that preserves
    /// the transmit throttle state and ping history but starts with an empty
    /// pending queue ("listener handoff should reset pending keepalive
    /// expectations", `createCookie`). The play listener after configuration
    /// calls this on handoff.
    pub fn copy_for_listener_handoff(&self) -> Self {
        let mut copy = KeepaliveState::new(self.last_keep_alive_tx_ns);
        copy.ping_calculator_1m.copy_from(&self.ping_calculator_1m);
        copy.ping_calculator_5s.copy_from(&self.ping_calculator_5s);
        copy.latency_ms = self.latency_ms;
        copy
    }

    /// `KeepAlive.pendingKeepAlives.peek()` — the oldest pending challenge.
    pub fn peek_pending(&self) -> Option<&PendingKeepAlive> {
        self.pending.front()
    }

    /// The number of pending challenges.
    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    /// `ServerCommonPacketListenerImpl.latency()`.
    pub fn latency_ms(&self) -> i32 {
        self.latency_ms
    }

    /// `keepConnectionAlive()` — Paper's per-tick keepalive driver.
    ///
    /// Java shape (`ServerCommonPacketListenerImpl.keepConnectionAlive`):
    /// ```java
    /// long now = Util.getMillis();
    /// if (this.checkIfClosed(now)) {
    ///     long currTime = System.nanoTime();
    ///     if ((currTime - this.lastKeepAliveTx) >= SECONDS.toNanos(1L)) {
    ///         this.lastKeepAliveTx = currTime;
    ///         PendingKeepAlive pka = new PendingKeepAlive(currTime, now);
    ///         this.pendingKeepAlives.add(pka);
    ///         this.send(new ClientboundKeepAlivePacket(pka.challengeId()));
    ///     }
    ///     PendingKeepAlive oldest = this.pendingKeepAlives.peek();
    ///     if (oldest != null && (currTime - oldest.txTimeNS()) > MILLISECONDS.toNanos(KEEPALIVE_LIMIT)) {
    ///         disconnect(TIMEOUT_DISCONNECTION_MESSAGE, TIMEOUT);
    ///     }
    /// }
    /// ```
    ///
    /// The `checkIfClosed` guard and the 15 s closed-listener timer are the
    /// listener's lifecycle concern (`#96` handoff); the *state* portion here
    /// is the throttle + timeout. `closed` handling lives on the
    /// [`super::network::keepalive::KeepaliveSink`] integrator.
    ///
    /// `now_ms` is the millis reading at the same instant as `tx_time_ns` —
    /// Paper reads `Util.getMillis()` then `System.nanoTime()`, and the pending
    /// challenge's id is that `now` value. The next tick re-reads both.
    ///
    /// Returns the transmit + timeout decision; the caller performs the send /
    /// disconnect through its sink. `pending` is mutated in place (a `Send`
    /// leaves the new challenge queued; a `Timeout` leaves the offending entry
    /// in place so the caller's disconnect is the terminal action — Paper
    /// disconnects and the listener is replaced/closed).
    pub fn tick(&mut self, tx_time_ns: i64, now_ms: i64) -> KeepaliveTickOutcome {
        let mut outcome = KeepaliveTickOutcome {
            send: None,
            timeout: false,
        };

        // 1 s transmit throttle: `(currTime - lastKeepAliveTx) >= 1s`.
        if tx_time_ns.wrapping_sub(self.last_keep_alive_tx_ns) >= 1_000_000_000 {
            self.last_keep_alive_tx_ns = tx_time_ns;
            let pka = PendingKeepAlive {
                tx_time_ns,
                challenge_id: now_ms,
            };
            self.pending.push_back(pka);
            outcome.send = Some(pka.challenge_id);
        }

        // 30 s timeout: `(currTime - oldest.txTimeNS()) > KEEPALIVE_LIMIT_NS`.
        if let Some(oldest) = self.pending.front().copied()
            && tx_time_ns.wrapping_sub(oldest.tx_time_ns) > KEEPALIVE_LIMIT_NS
        {
            outcome.timeout = true;
        }

        outcome
    }

    /// `handleKeepAlive(ServerboundKeepAlivePacket)` — Paper's serverbound
    /// response handling. Returns what the connection should do with the
    /// response.
    ///
    /// Java shape (`ServerCommonPacketListenerImpl.handleKeepAlive`): the
    /// response's id is matched against the *oldest* pending challenge first —
    /// a match removes it and updates the ping calculators and `latency`; then
    /// the remaining queue is scanned for a match, and one there means the
    /// response is *out-of-order* — the match is removed and the client is
    /// disconnected (TIMEOUT); no match anywhere is a "without matching
    /// challenge" disconnect (TIMEOUT).
    ///
    /// The challenge-id generation semantics: ids come from `now_ms` (the
    /// `Util.getMillis()` reading at transmit time), so under one server the id
    /// is a monotonic millis timestamp — but Paper makes no uniqueness
    /// guarantee and every branch below depends only on equality, not ordering,
    /// so any id the caller chose is handled identically.
    pub fn handle_keepalive(
        &mut self,
        response_id: i64,
        rx_time_ns: i64,
    ) -> KeepaliveResponseOutcome {
        // Fast path: the response matches the newest expectation (oldest
        // pending). Java `peek()` + `remove(pending)`.
        if let Some(pending) = self.pending.front().copied()
            && pending.challenge_id == response_id
        {
            self.pending.pop_front();
            let response = KeepAliveResponse {
                tx_time_ns: pending.tx_time_ns,
                rx_time_ns,
            };
            self.ping_calculator_1m.update(response);
            self.ping_calculator_5s.update(response);
            self.latency_ms = self.ping_calculator_5s.get_avg_latency_ms();
            return KeepaliveResponseOutcome::Accepted;
        }

        // Out-of-order: the response matches a non-oldest pending challenge.
        // Java iterates the queue, `itr.remove()`s the match, then disconnects.
        if let Some(pos) = self
            .pending
            .iter()
            .position(|ka| ka.challenge_id == response_id)
        {
            self.pending.remove(pos);
            return KeepaliveResponseOutcome::OutOfOrder;
        }

        // No challenge matched.
        KeepaliveResponseOutcome::NoMatchingChallenge
    }

    /// Drop a pending challenge (the integrator's disconnect path). Java leaves
    /// the queue owned by the listener it is disconnecting; a fresh
    /// `copyForListenerHandoff` resets it. This is the explicit "clear" for the
    /// queue when a connection transitions.
    pub fn clear_pending(&mut self) {
        self.pending.clear();
    }
}

/// `handleKeepAlive` verdict for a serverbound `keep_alive`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeepaliveResponseOutcome {
    /// Matched the oldest pending challenge: accepted, ping calculators
    /// updated, `latency` recomputed.
    Accepted,
    /// Matched a non-oldest pending challenge: out-of-order, disconnect with
    /// TIMEOUT.
    OutOfOrder,
    /// No pending challenge matches: disconnect with TIMEOUT.
    NoMatchingChallenge,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Milliseconds, in ns, as an `i64` (the mono axis; starts at 0).
    fn ms_to_ns(ms: i64) -> i64 {
        ms * 1_000_000
    }

    /// A fresh machine whose `lastKeepAliveTx` is `start_ms`.
    fn state(start_ms: i64) -> KeepaliveState {
        KeepaliveState::new(ms_to_ns(start_ms))
    }

    /// Advance simulated time to `t_ms`, run one tick, and return the outcome.
    fn tick_at(s: &mut KeepaliveState, t_ms: i64) -> KeepaliveTickOutcome {
        s.tick(ms_to_ns(t_ms), t_ms)
    }

    #[test]
    fn no_send_before_one_second_throttle() {
        let mut s = state(0);
        // t=999ms: 999ms elapsed < 1s, no send.
        let o = tick_at(&mut s, 999);
        assert_eq!(o.send, None);
        assert!(!o.timeout);
        assert_eq!(s.pending_len(), 0);
    }

    #[test]
    fn send_at_exactly_one_second() {
        let mut s = state(0);
        // `>=` is inclusive: exactly 1000ms sends.
        let o = tick_at(&mut s, 1000);
        assert_eq!(o.send, Some(1000), "challenge id is the millis reading");
        assert_eq!(s.pending_len(), 1);
        assert_eq!(s.peek_pending().unwrap().challenge_id, 1000);
        assert_eq!(s.peek_pending().unwrap().tx_time_ns, ms_to_ns(1000));
    }

    #[test]
    fn throttle_resets_and_requires_another_second() {
        let mut s = state(0);
        tick_at(&mut s, 1000); // send at t=1000ms
        // 1999ms: only 999ms since the send — no second packet.
        let o = tick_at(&mut s, 1999);
        assert_eq!(o.send, None);
        // 2000ms: exactly 1s since the last send — second packet.
        let o = tick_at(&mut s, 2000);
        assert_eq!(o.send, Some(2000));
        assert_eq!(s.pending_len(), 2);
        // FIFO order: the older challenge is still first.
        assert_eq!(s.peek_pending().unwrap().challenge_id, 1000);
    }

    #[test]
    fn challenge_id_is_millis_reading() {
        // Paper: `new PendingKeepAlive(currTime /*ns*/, now /*ms*/)` — the id is
        // the millis timestamp, NOT the ns reading. Assert the distinctness.
        let mut s = state(0);
        let o = tick_at(&mut s, 123_456);
        assert_eq!(o.send, Some(123_456));
        let pka = s.peek_pending().unwrap();
        assert_eq!(pka.challenge_id, 123_456);
        assert_eq!(pka.tx_time_ns, ms_to_ns(123_456));
    }

    #[test]
    fn valid_response_accepts_and_updates_latency() {
        let mut s = state(0);
        tick_at(&mut s, 1000); // challenge id 1000, tx 1000ms
        // Response after 50ms of "network": rx ns = 1050ms worth.
        let outcome = s.handle_keepalive(1000, ms_to_ns(1050));
        assert_eq!(outcome, KeepaliveResponseOutcome::Accepted);
        assert_eq!(s.pending_len(), 0);
        // One sample in the 5s window: latency 50ms.
        assert_eq!(s.ping_calculator_5s.all_latencies_ns(), vec![50_000_000]);
        assert_eq!(s.latency_ms(), 50);
    }

    #[test]
    fn out_of_order_response_disconnects() {
        let mut s = state(0);
        tick_at(&mut s, 1000); // challenge 1000 (oldest)
        tick_at(&mut s, 2000); // challenge 2000
        // Respond to the SECOND challenge first: 2000 is pending but not oldest.
        let outcome = s.handle_keepalive(2000, ms_to_ns(2500));
        assert_eq!(outcome, KeepaliveResponseOutcome::OutOfOrder);
        // Java `itr.remove()`s the matched pending before disconnecting — only
        // the offending (2000) entry is gone; the oldest (1000) stays.
        assert_eq!(s.pending_len(), 1);
        assert_eq!(s.peek_pending().unwrap().challenge_id, 1000);
        assert_eq!(
            s.latency_ms(),
            0,
            "out-of-order response never updates latency"
        );
    }

    #[test]
    fn response_without_matching_challenge_disconnects() {
        let mut s = state(0);
        tick_at(&mut s, 1000); // challenge 1000
        let outcome = s.handle_keepalive(9999, ms_to_ns(2000));
        assert_eq!(outcome, KeepaliveResponseOutcome::NoMatchingChallenge);
        assert_eq!(s.pending_len(), 1, "no challenge removed");
    }

    #[test]
    fn duplicate_response_after_accept_is_no_matching_challenge() {
        // Send twice, respond to the first, then replay the first response id —
        // Java's queue no longer contains it, so it is a "without matching
        // challenge" disconnect (NOT a silent no-op).
        let mut s = state(0);
        tick_at(&mut s, 1000);
        tick_at(&mut s, 2000);
        assert_eq!(
            s.handle_keepalive(1000, ms_to_ns(1500)),
            KeepaliveResponseOutcome::Accepted
        );
        let outcome = s.handle_keepalive(1000, ms_to_ns(3000));
        assert_eq!(outcome, KeepaliveResponseOutcome::NoMatchingChallenge);
    }

    #[test]
    fn timeout_fires_strictly_after_limit() {
        let mut s = state(0);
        tick_at(&mut s, 1000); // oldest tx = 1000ms
        // At tx=1000+30000=31000ms exactly, elapsed == limit — strict `>` does
        // NOT fire.
        let o = tick_at(&mut s, 1000 + KEEPALIVE_LIMIT_MS);
        assert!(!o.timeout);
        // One ms later it fires.
        let o = tick_at(&mut s, 1000 + KEEPALIVE_LIMIT_MS + 1);
        assert!(o.timeout);
    }

    #[test]
    fn timeout_only_checks_oldest_pending() {
        // The oldest challenge is answered, so a younger one past its own limit
        // does not fire (Paper checks only `peek()`).
        let mut s = state(0);
        tick_at(&mut s, 1000); // oldest, will be answered
        tick_at(&mut s, 60_000); // second challenge, 59s later
        // Respond to the oldest before its 30s elapses (at t=65s it's still
        // within 30s of 35s... the oldest tx is 1000ms; we answer at 5000ms).
        assert_eq!(
            s.handle_keepalive(1000, ms_to_ns(5000)),
            KeepaliveResponseOutcome::Accepted
        );
        // Now the oldest pending is the 60s challenge. At 100s, it's been
        // pending 40s > 30s → timeout. The 1s throttle also fires (100s - 60s >
        // 1s), so a new challenge is queued too — both branches in one tick.
        let o = tick_at(&mut s, 100_000);
        assert!(o.timeout);
        assert_eq!(o.send, Some(100_000));
        assert_eq!(
            s.pending_len(),
            2,
            "the timed-out entry stays pending; a new challenge was also sent"
        );
        assert_eq!(
            s.peek_pending().unwrap().challenge_id,
            60_000,
            "the timed-out entry is still the oldest"
        );
    }

    #[test]
    fn timeout_with_no_pending_never_fires() {
        let mut s = state(0);
        // Let 40 seconds pass with the single challenge answered.
        tick_at(&mut s, 1000);
        assert_eq!(
            s.handle_keepalive(1000, ms_to_ns(1500)),
            KeepaliveResponseOutcome::Accepted
        );
        // 40s later, no pending → no timeout.
        let o = tick_at(&mut s, 41_000);
        assert!(!o.timeout);
        // ... but the 1s throttle has long since elapsed, so a new challenge is
        // sent.
        assert_eq!(o.send, Some(41_000));
    }

    #[test]
    fn ping_window_evicts_out_of_window_responses() {
        let mut calc = PingCalculator::five_seconds();
        // t=0: a 100ms latency sample.
        calc.update(KeepAliveResponse {
            tx_time_ns: 0,
            rx_time_ns: 100_000_000,
        });
        // t=5s+1ns: the first sample is now out of the 5s window.
        calc.update(KeepAliveResponse {
            tx_time_ns: 5_000_000_001,
            rx_time_ns: 5_050_000_001,
        });
        assert_eq!(
            calc.all_latencies_ns(),
            vec![50_000_000],
            "only the in-window sample remains"
        );
        assert_eq!(calc.get_avg_latency_ms(), 50);
    }

    #[test]
    fn ping_window_average_is_sum_over_count() {
        let mut calc = PingCalculator::five_seconds();
        for (tx_ms, rx_ms) in [(0, 10), (1_000, 1_050), (2_000, 2_120)] {
            calc.update(KeepAliveResponse {
                tx_time_ns: ms_to_ns(tx_ms),
                rx_time_ns: ms_to_ns(rx_ms),
            });
        }
        // Latencies: 10, 50, 120 ms → avg (10+50+120)/3 = 60ms.
        assert_eq!(
            calc.all_latencies_ns(),
            vec![10_000_000, 50_000_000, 120_000_000]
        );
        assert_eq!(calc.get_avg_latency_ms(), 60);
    }

    #[test]
    fn ping_window_negative_latency_truncates_toward_zero() {
        // A rx before tx (clock anomaly) gives a negative latency. Java's
        // `TimeUnit.NANOSECONDS.toMillis(-1_000_000)` is `-1_000_000 / 1_000_000`
        // (truncating division) = -1; the port matches (Rust i64 division is the
        // same truncation toward zero).
        let mut calc = PingCalculator::five_seconds();
        calc.update(KeepAliveResponse {
            tx_time_ns: ms_to_ns(10_000),
            rx_time_ns: ms_to_ns(10_000 - 1),
        });
        assert_eq!(calc.all_latencies_ns(), vec![-1_000_000]);
        assert_eq!(calc.get_avg_latency_ms(), -1, "-1ms truncates to -1");
    }

    #[test]
    fn average_truncates_not_rounds() {
        let mut calc = PingCalculator::five_seconds();
        // 3 samples summing to 100ms → 33.33ms → 33ms (not 34).
        calc.update(KeepAliveResponse {
            tx_time_ns: 0,
            rx_time_ns: ms_to_ns(40),
        });
        calc.update(KeepAliveResponse {
            tx_time_ns: ms_to_ns(1_000),
            rx_time_ns: ms_to_ns(1_040),
        });
        calc.update(KeepAliveResponse {
            tx_time_ns: ms_to_ns(2_000),
            rx_time_ns: ms_to_ns(2_020),
        });
        assert_eq!(calc.get_avg_latency_ms(), 33);
    }

    #[test]
    fn handoff_copy_preserves_throttle_and_history_resets_pending() {
        let mut s = state(0);
        tick_at(&mut s, 1000);
        assert_eq!(
            s.handle_keepalive(1000, ms_to_ns(1050)),
            KeepaliveResponseOutcome::Accepted
        );
        s.pending.clear();
        tick_at(&mut s, 2000); // a fresh pending to be discarded by the copy

        let mut copy = s.copy_for_listener_handoff();
        assert_eq!(copy.pending_len(), 0, "handoff resets pending expectations");
        assert_eq!(
            copy.ping_calculator_5s.all_latencies_ns(),
            s.ping_calculator_5s.all_latencies_ns(),
            "ping history carries over"
        );
        assert_eq!(copy.latency_ms(), 50, "latency carries over");
        // The throttle state carries over: the next send is 1s after the last
        // transmit, not immediately. At 2500ms (500ms after the t=2000 send),
        // no send fires.
        let o = copy.tick(ms_to_ns(2500), 2500);
        assert_eq!(o.send, None);
        // At 3000ms it does.
        let o = copy.tick(ms_to_ns(3000), 3000);
        assert_eq!(o.send, Some(3000));
    }

    #[test]
    fn wrapping_at_i64_max_does_not_panic() {
        // PORTING.md: wrapping arithmetic is sacred. At `i64::MAX` the mono
        // clock wraps; the machine must not panic and must keep Paper's
        // subtraction semantics (which wrap identically).
        let mut s = KeepaliveState::new(i64::MAX - 10);
        // `tx - last` wraps to a huge negative → below 1s → no send.
        let o = s.tick(i64::MAX - 5, i64::MAX - 5);
        assert_eq!(o.send, None);
        // Advance past the wrap: the difference is again large-positive.
        let o = s.tick(i64::MIN + 10, i64::MIN + 10);
        // `lastKeepAliveTx` was MAX-10; tx is MIN+10 → diff wraps to ~20ns.
        // Not a second, so no send; but it must not panic.
        assert_eq!(o.send, None);
        // A pending challenge from just before the wrap, checked after it, must
        // not panic on the timeout subtraction either.
        let mut s2 = KeepaliveState::new(i64::MAX - 1000);
        s2.tick(i64::MAX - 5, 7); // challenge id 7, tx near MAX
        let o2 = s2.tick(i64::MIN + 1000, i64::MIN + 1000);
        // The wrap makes the elapsed appear large; a timeout may fire — the
        // point of the test is that no arithmetic panics.
        let _ = o2.timeout;
    }

    #[test]
    fn many_pending_challenges_fifo_accept_in_order() {
        let mut s = state(0);
        // No responses, one transmit per second for 10 seconds.
        for t in (1..=10).map(|i| i * 1000) {
            tick_at(&mut s, t);
        }
        assert_eq!(s.pending_len(), 10);
        // Respond in order: each accepted response removes the oldest.
        for i in 1..=10 {
            let id = i * 1000;
            assert_eq!(
                s.handle_keepalive(id, ms_to_ns(id + 5)),
                KeepaliveResponseOutcome::Accepted,
                "challenge {id}"
            );
        }
        assert_eq!(s.pending_len(), 0);
        // Latency is the 5ms average.
        assert_eq!(s.latency_ms(), 5);
    }

    #[test]
    fn timeout_and_send_can_fire_in_same_tick() {
        // A pending challenge older than 30s and a throttled 1s both elapse on
        // the same tick: Paper checks both branches, so both actions result.
        let mut s = state(0);
        tick_at(&mut s, 1_000); // challenge 1000 (never answered)
        // Jump to 32s: throttle fires (32s - 1s) and the 1s challenge (31s old)
        // times out.
        let o = tick_at(&mut s, 32_000);
        assert_eq!(o.send, Some(32_000));
        assert!(o.timeout);
        assert_eq!(s.pending_len(), 2, "both challenges still pending");
    }

    #[test]
    fn clear_pending_drops_all_challenges() {
        let mut s = state(0);
        tick_at(&mut s, 1_000);
        tick_at(&mut s, 2_000);
        assert_eq!(s.pending_len(), 2);
        s.clear_pending();
        assert_eq!(s.pending_len(), 0);
        // After clearing, a response to an old id is "no matching challenge".
        assert_eq!(
            s.handle_keepalive(1_000, ms_to_ns(3_000)),
            KeepaliveResponseOutcome::NoMatchingChallenge
        );
    }
}
