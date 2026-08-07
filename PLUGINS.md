# PLUGINS.md — plugin compatibility tiers

The canonical reference for what "Paper plugins run on Rivet" means. Binding decision
`DECISIONS.md` D14; mechanics live in `D6` (JVM adapter) and the corpus epics (#27, #250, M4
acceptance #251, M5 recon #252 / adapter hardening #253 / terminal gate #254, Tier 3 gate #255).
Read this before touching anything plugin-shaped; propose changes as dedicated PRs (D9).

## Why tiers, not a binary promise

"Drop-in Paper plugins" is a **measured compatibility level**, never a universal claim. The Bukkit/Paper
ecosystem spans clean public-API plugins, plugins that talk to other plugins, and plugins that reach into
NMS/CraftBukkit internals or intercept the protocol. Those need wildly different adapter work, so Rivet
slices them into tiers and publishes **corpus pass rates** per tier. A blanket "loads unchanged" promise
(README used to say exactly that) is exactly what this model removes.

## The three tiers

### Tier 1 — API-clean (M4)

Public Paper/Bukkit API compatibility through the JVM adapter (D6), **measured by a curated corpus** of
behavior-diverse, API-clean plugins. Explicitly NOT universal plugin compatibility: NMS/reflection and
internals are out of scope. This is the M4 acceptance gate (#251): each corpus plugin boots and passes its
smoke script on the pinned Paper 26.2 baseline and on Rivet, and the comparator detects a tampered API
result (controlled negative).

### Tier 2 — ecosystem compatibility (M5)

Plugin-to-plugin/service compatibility, scheduler-async semantics, Adventure components, library loading
(`paper-libraries` / custom), and custom classloaders — each measured against the **broad Tier 2 corpus**
pass rates. Recon (#252) records each plugin's observable behavior on Paper 26.2 to scope the adapter
hardening (#253); the terminal gate (#254) publishes per-plugin and per-category pass rates plus a
performance dashboard (Rust tick / FFI / Java plugin CPU / Java allocations / Java GC). This closes M5.

### Tier 3 — targeted CraftBukkit/internal compatibility, decision-gated

A **named, evidence-driven** set of CraftBukkit/internal entry points that high-corpus-value plugins
actually hit, discovered from M4/M5 corpus data — not an assumed list. Each candidate gets a
cost/benefit record; the decision gate (#255) stays **blocked until the evidence exists**, and a decision
is recorded before any Tier 3 implementation milestone may exist.

**Universal NMS is explicitly never pursued.** "Run any NMS plugin" is out of scope in every tier, and
ProtocolLib-style interception (bytecode/network interception at the CraftBukkit level) requires a
separate decision — it is not authorized by the Tier 3 gate.

## How compatibility is measured

- **Corpus**, not unit tests: each plugin is pinned, its observable lifecycle/events/API interactions are
  recorded on pinned Paper 26.2 (the baseline), then the same boot + smoke runs against Rivet. The corpus
  tells the adapter what to build (epic #27 / #250 concern 9), so tiers stay evidence-driven.
- **Scoreboard rows** per plugin published in `PARITY.md`. That file is generated wholesale by
  `rivet-parity --scoreboard`, so the corpus table is emitted by the generator — never hand-edited, since the
  next `--scoreboard` run overwrites the whole file. The scoreboard **row** schema is defined in the generator
  (`plugin_corpus_section` in `tools/rivet-parity/src/main.rs`), not in this file; it is distinct from the
  corpus **manifest** schema below. The generator already emits the planned stub; real corpus rows are added
  at M4 wiring. Plus per-category pass rates at M5.
- **Exit-code contract** (mirrors the oracle steps in `scripts/gate.sh`): 0 = VERIFIED (Rivet == Paper
  baseline), 1 = FAILED (a real diff), 3 = UNVERIFIED (missing artifact or prereq — never silently skipped).
- **Controlled negatives**: tampering one API result must be detected by the comparator — a fake green
  (`--expect-fail`-style) is not acceptable (D8).

## The M4 checkpoint

The Tier 1 corpus starts as a **manifest-driven, offline, sandboxed sweep** — defined here, executed under
epic #27:

1. **~20–30 popular plugins selected by behavior category**, not popularity alone (event-heavy,
   command/Adventure, inventory, world-editing, permissions/services, scheduler-heavy; NMS/reflection
   plugins are negative controls, not targets). Seed candidates from #27: LuckPerms, Vault,
   PlaceholderAPI, EssentialsX (core), WorldGuard (stretch).
2. **Every plugin pinned**: version + download URL + SHA-256 + license recorded in the corpus manifest
   (`tools/rivet-oracle/` — schema below). **No pin, no run**: a missing artifact is UNVERIFIED (exit 3).
3. **Never commit third-party JARs.** They are gitignored (`*.jar` already is) and fetched on demand;
   execution is offline/sandboxed once fetched. Nothing third-party enters the repo.
4. **Paper baseline first.** Each plugin's boot + smoke runs on pinned Paper 26.2 and its observable
   behavior is recorded before any Rivet claim. Rivet smoke scripts run only when the adapter (#26) can
   actually load the plugin; until then the checkpoint is the baseline recording, honestly labeled.

The corpus **manifest** schema (the sweep's input pinning file; planned — no real pins exist until the
sweep runs). This is distinct from the scoreboard **row** schema, which the generator emits into
`PARITY.md` (`plugin_corpus_section` in `tools/rivet-parity/src/main.rs`):

```
plugin    category            version   source_url   sha256   license   paper_baseline   rivet
# e.g.
LuckPerms permissions/services  <pin>    <url>        <sha>    <lic>     recorded          pending
```

`paper_baseline`/`rivet` start as `recorded`/`pending` (or `n/a`); no row is a result until the sweep
produced it.

## Out of scope / deferred

- Universal NMS compatibility — never (Tier 3, above).
- ProtocolLib-style interception — separate decision, not authorized by this model.
- Native Rust `rivet-api` (#28) — a separate surface, tracked independently, not part of any tier's corpus.
