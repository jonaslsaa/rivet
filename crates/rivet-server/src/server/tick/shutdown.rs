//! Orderly shutdown signal shared between the tick thread (std `Condvar` sleep,
//! so it can be interrupted mid-tick-wait) and the tokio side (accept loop
//! awaiting the same event).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::Duration;

use tokio::sync::watch;

/// A one-way stop signal. `request()` is idempotent; the tick loop polls
/// `is_requested()` each iteration and its idle sleep is woken by the condvar.
#[derive(Debug)]
pub struct Shutdown {
    flag: AtomicBool,
    lock: Mutex<()>,
    condvar: Condvar,
    /// Async wake for the accept loop (`wait_async`).
    watch_tx: watch::Sender<bool>,
}

impl Shutdown {
    pub fn new() -> Self {
        let (watch_tx, _) = watch::channel(false);
        Shutdown {
            flag: AtomicBool::new(false),
            lock: Mutex::new(()),
            condvar: Condvar::new(),
            watch_tx,
        }
    }

    /// Request shutdown. Idempotent; safe to call from any thread.
    pub fn request(&self) {
        // Taking the lock while setting the flag and notifying closes the
        // classic lost-wakeup race: a waiter that checked the flag under this
        // lock is guaranteed to observe the notify.
        let _guard = self.lock.lock().unwrap();
        self.flag.store(true, Ordering::SeqCst);
        self.condvar.notify_all();
        let _ = self.watch_tx.send(true);
    }

    pub fn is_requested(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }

    /// Sleep for `dur`, or until `request()` fires. Returns whether shutdown was
    /// requested (the caller re-checks and stops).
    pub fn sleep_for(&self, dur: Duration) -> bool {
        let guard = self.lock.lock().unwrap();
        if self.is_requested() {
            return true;
        }
        let (guard, _) = self
            .condvar
            .wait_timeout(guard, dur)
            .unwrap_or_else(|e| e.into_inner());
        drop(guard);
        self.is_requested()
    }

    /// An async wait for the tokio side (e.g. the accept loop), so it stops
    /// promptly without busy-polling.
    pub async fn wait_async(&self) {
        let mut rx = self.watch_tx.subscribe();
        if *rx.borrow() {
            return;
        }
        let _ = rx.changed().await;
    }
}

impl Default for Shutdown {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_is_idempotent_and_observable() {
        let s = Shutdown::new();
        assert!(!s.is_requested());
        s.request();
        assert!(s.is_requested());
        s.request();
        assert!(s.is_requested());
    }

    #[test]
    fn sleep_returns_early_on_shutdown() {
        let s = std::sync::Arc::new(Shutdown::new());
        let t = s.clone();
        let sleeper = std::thread::spawn(move || {
            // A sleep much longer than the test should ever take.
            t.sleep_for(Duration::from_secs(60))
        });
        std::thread::sleep(Duration::from_millis(20));
        s.request();
        assert!(sleeper.join().unwrap(), "sleep must observe the shutdown");
    }

    #[test]
    fn sleep_returns_after_duration() {
        let s = std::sync::Arc::new(Shutdown::new());
        let t = s.clone();
        let start = std::time::Instant::now();
        let slept = t.sleep_for(Duration::from_millis(10));
        assert!(start.elapsed() >= Duration::from_millis(5));
        assert!(!slept, "no shutdown was requested");
    }
}
