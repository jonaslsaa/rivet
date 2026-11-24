# Rivet Workflows

How the port is driven with Claude Code workflows. Background and rationale: `docs/background.md`. Roadmap context: `ROADMAP.md` section 3. The merge gate is `scripts/gate.sh`; there is no hosted CI (D10).

## Rules that hold for every workflow

- **Durable state lives in git.** `MANIFEST.tsv` and the ledgers (`docs/ledgers.md`) are the work queue. Workflows are stateless executors; the controller (main session) selects rows, passes them in as args, and updates the manifest. Workflows never read `MANIFEST.tsv` themselves.
- **Launch with `scriptPath`.** Run each workflow by passing the absolute path of its script, `<main checkout>/.claude/workflows/<name>.js`, as `scriptPath`, with the args object below. Do not rely on launching by name.
- **Pin the worktree.** Workflow agents inherit the session cwd, so every lane workflow takes the absolute `worktree` path and pins it in every prompt. That lets several lanes run at once, one worktree and branch per lane. Paper sources are read from the main checkout's `working/Paper`.
- **Agents commit, the controller pushes.** No workflow pushes, opens a PR or merges. Agents never run `git stash`, `git reset`, rebase or force-push, and never weaken a test, fixture or the oracle to go green.
- **Fresh reviewers.** Reviewers are new instances each round, see code and Java originals rather than the implementer's reasoning, and return schema-forced verdicts. The implementer never judges its own convergence.
- **Fix the process, not the output.** If the same bug class appears in two units, edit `PORTING.md` or the prompt and re-run instead of hand-patching.
- **Tests (D18).** Only ported Paper JUnit tests, tests backed by a Java-generated fixture, and tickdiff or client scenarios. Do not invent behavioural unit tests.

## Scripts (`.claude/workflows/`)

`?` marks an optional arg; defaults follow `=`.

| Script | Args | Behaviour |
|---|---|---|
| `scaffold` | `{worktree, family, javaPaths, crate, modulePath, implModel?='sonnet', implEffort?='high'}` | One agent writes a family's mirrored files, `mod` lines and type/fn signatures with `STUB(<unit-id>)` bodies from the Java; one fresh reviewer per round checks the surface against the Java and `cargo check -p <crate>`. At most 2 rounds. Commits on the lane branch. The scaffold PR merges alone, before the family's leaf waves. `javaPaths` are manifest `root:relpath` entries; `modulePath` is the Rust module inside `crates/<crate>/src`. |
| `translate-wave` | `{waveId, worktree, units: [{id, java_package, java_paths, crate, notes?}], rigor?='full', implModel?='sonnet', implEffort?='high', reviewModel?=session model, reviewEffort?='high'}` | Per unit: implement, then a review and fix loop. `rigor: full` runs two lenses in round 1 (semantic drift; ownership, API and canonical types), then one reviewer per round, at most 3 rounds. `lean` runs one combined reviewer per round, at most 2. Reviewers run `cargo check -p <crate>`; errors in the unit's files are critical. Converged means zero findings and a clean check. Minor-only findings get one blind fix; critical or major after the last round means `blocked`. Units run as a pipeline, and agents commit per unit by explicit path. |
| `check-burndown` | `{worktree, maxRounds?=8, fixModel?='sonnet', fixEffort?='high'}` | Runs workspace `cargo check`, partitions errors by module, spawns one fixer per partition, and loops until clean or the round cap. Fixers correct the translation; they never delete functionality, and a `todo!()` needs a `blocked` note. |
| `worktree-task` | `{worktree, task, maxRounds?=3, commit?=true, implModel?, implEffort?}` | For substantive non-translation work (tooling, harnesses, codegen, infra). One implementer, then fresh scoped reviewers each round, then a fixer. From round 2 reviewers get the prior findings and the fixer's disputes, but only after their own fresh pass. `implModel` and `implEffort` apply to the implementer and fixer only; reviewers run at high effort on the session model. `commit` must agree with the task text. |
| `release-prep` | `{worktree, task, maxRounds?=6, checks?}` | For an already-implemented branch. Each round fetches and **merges `origin/main` into the branch, never rebases** (the branch may be a pushed PR head), runs focused checks, and hands the exact head to a fresh read-only reviewer; confirmed findings go to a fixer that commits locally. Fail-closed: reviewed and checked hashes must match, at least one check command must have run, every changed file must be inspected, the tree must be clean, the branch ahead of and zero behind `origin/main`, and findings empty. Success reports `strict_gate_ready: true` with head, base, check and review evidence. It never runs `scripts/gate.sh`, pushes, opens a PR or merges. |
| `review-pr` | `{pr}` or `{range}`, plus `effort?='high'` (`low`, `medium`, `high`, `xhigh`, `max`) | One whole-diff reviewer, run once per PR after `check-burndown`. It looks for what per-unit review cannot: cross-unit stub conflicts, aggregate `OWNERSHIP.md` violations, API-surface incoherence, and process violations (weakened tests, smuggled doc edits). Use `max` for spine PRs and train reviews. The controller then runs `scripts/gate.sh`. |

Every script accepts `args` either as an object or as a JSON string.

## Controller cycle

1. Pick ready units from the ledger and `MANIFEST.tsv`: units whose deps are `done`, with the family scaffold merged. `scripts/wave_picker.py` helps; the controller builds the args.
2. `scaffold` once per family, then `translate-wave` per wave in a lane worktree, then `check-burndown` if the workspace does not compile.
3. `release-prep` to bring the branch onto current `origin/main` with an exact-head review, then `review-pr`, then the serialized `scripts/gate.sh` (run with `--require-oracle` when the oracle must not be skippable). A red gate blocks the merge.
4. Update `MANIFEST.tsv` and the ledgers in the same PR. Keep `MANIFEST.tsv` edits to generated fields; regenerate with `scripts/analyze_graph.py` (flags in `CLAUDE.md`).
5. Unit status flows `pending`, `translated`, `reviewed`, `fixed`, `done`, plus `blocked(reason)` for controller triage. Blocked units and units that failed three times stop retrying and surface for a decision.

Parallelism: within a wave there are no worktrees (disjoint files in one checkout); across lanes there is one worktree and branch per lane; shared-file hotspots (`Cargo.toml`, `lib.rs` mod lists, `MANIFEST.tsv`) are edited only by the controller, or in a tiny standalone PR first.

## Comment markers: `STUB` and `RivetTodo`

`scripts/check_markers.py` enforces both forms in the gate.

- `// STUB(<manifest-unit-id>) <reason>`: no implementation here. The id must exist and must not be `done`; a STUB on a done unit is stale.
- `// RivetTodo(#<issue>) <reason>`: usable partial port with a tracked gap. The argument is a GitHub issue number.

A line is a marker only when its body begins with the token, the reason is mandatory, both forms on one line is an error, and every `todo!()` or `unimplemented!()` needs a `RivetTodo` on the same line or the line above. On a `done` unit every remaining gap is a `RivetTodo`.

## Reviewer drift checklist

The Java to Rust semantic-drift list (wrapping arithmetic, `>>` vs `>>>`, HashMap order, UTF-16 vs UTF-8, `Mth` tables and float parity, dropped null checks, equality semantics, `synchronized`, eager vs lazy evaluation) lives in `PORTING.md` and is pasted into every review prompt.
