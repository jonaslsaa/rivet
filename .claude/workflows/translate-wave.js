export const meta = {
  name: 'translate-wave',
  description: 'Translate a wave of Rivet manifest units from Java to Rust in one lane worktree, looping adversarial review until clean',
  whenToUse: 'One port wave in one lane worktree. Pass args {waveId, worktree, units: [{id, java_package, java_paths, crate, notes?}], rigor?, implModel?, implEffort?, reviewModel?, reviewEffort?} built from MANIFEST.tsv rows by the controller. worktree is the absolute path of the lane worktree (its branch already checked out); java_paths are the manifest root:relpath entries (array or the comma-joined manifest cell). rigor "full" (default: two-lens first round, at most 3 review rounds) or "lean" (one combined reviewer per round, at most 2 rounds). Implementers and fixers run on implModel/implEffort (default sonnet/high); reviewers on reviewModel (default: session model) / reviewEffort (default high). A unit converges only with zero findings and no cargo check -p <crate> errors in its files; agents commit per unit on the lane branch and never push.',
  phases: [{ title: 'Translate' }, { title: 'Review' }, { title: 'Fix' }, { title: 'Check' }],
}

// args sometimes arrives JSON-encoded as a string instead of an object.
const A = typeof args === 'string' ? JSON.parse(args) : args

if (!A || !A.worktree || !Array.isArray(A.units) || A.units.length === 0) {
  throw new Error('translate-wave requires args {waveId, worktree, units: [...]}')
}
// Workflow agents inherit the SESSION cwd, not the lane worktree, so every
// prompt pins this absolute path; that is what lets several waves run at once
// in separate worktrees.
const WT = String(A.worktree).replace(/\/+$/, '')
if (!WT.startsWith('/')) throw new Error(`translate-wave: worktree must be an absolute path (got ${WT})`)
// Lane worktrees live at <main checkout>/.claude/worktrees/<name>; the Paper
// source tree is the main checkout's working/Paper (never copied per worktree).
const WT_MARK = '/.claude/worktrees/'
const MAIN = WT.includes(WT_MARK) ? WT.slice(0, WT.indexOf(WT_MARK)) : WT
const PAPER = `${MAIN}/working/Paper`
// MANIFEST.tsv java_paths roots (scripts/analyze_graph.py SOURCE_ROOTS).
const SOURCE_ROOTS = {
  minecraft: `${PAPER}/paper-server/src/minecraft/java`,
  'paper-server': `${PAPER}/paper-server/src/main/java`,
  'paper-api': `${PAPER}/paper-api/src/main/java`,
}

const RIGOR = A.rigor || 'full'
if (RIGOR !== 'lean' && RIGOR !== 'full') throw new Error(`translate-wave: rigor must be "lean" or "full" (got ${RIGOR})`)
const MAX_REVIEW_ROUNDS = RIGOR === 'lean' ? 2 : 3

// Mostly-mechanical porting runs on Sonnet high (owner rule); reviewers inherit
// the session model unless the controller names one.
const IMPL_OPTS = { model: A.implModel || 'sonnet', effort: A.implEffort || 'high' }
const REVIEW_OPTS = A.reviewModel
  ? { model: A.reviewModel, effort: A.reviewEffort || 'high' }
  : { effort: A.reviewEffort || 'high' }

function resolveJavaPaths(u) {
  const raw = Array.isArray(u.java_paths) ? u.java_paths : String(u.java_paths || '').split(',')
  const entries = raw.map((e) => String(e).trim()).filter(Boolean)
  if (entries.length === 0) throw new Error(`translate-wave: unit ${u.id} has no java_paths`)
  return entries.map((e) => {
    const i = e.indexOf(':')
    const root = i > 0 ? e.slice(0, i) : ''
    const rel = i > 0 ? e.slice(i + 1) : ''
    if (!SOURCE_ROOTS[root] || !rel.endsWith('.java')) {
      throw new Error(`translate-wave: unit ${u.id} has a malformed java_paths entry "${e}" (want root:relpath, root one of ${Object.keys(SOURCE_ROOTS).join('/')})`)
    }
    return `${SOURCE_ROOTS[root]}/${rel}`
  })
}

const UNITS = A.units.map((u) => {
  if (!u || !u.id || !u.crate) throw new Error('translate-wave: every unit needs id, crate, and java_paths')
  return { ...u, javaFiles: resolveJavaPaths(u) }
})

// A stalled provider call must cost one retry, not a silently dropped unit:
// agent() throws on harness-level stall exhaustion and returns null on terminal
// API errors. One extra attempt only — the harness already retries internally.
// Never retry a user abort. The retry preamble makes the attempt a distinct
// journal key, so a resumed run never replays a half-dead attempt as completed.
async function resilientAgent(prompt, opts, attempts = 2) {
  for (let attempt = 1; attempt <= attempts; attempt++) {
    let attemptPrompt = prompt
    if (attempt > 1) {
      attemptPrompt =
        `(Retry ${attempt}/${attempts}: the previous attempt did not complete — stall, provider error, or interruption. ` +
        `Redo the task from the current on-disk state; if your task writes files, reconcile any partial changes from ` +
        `the prior attempt rather than assuming a clean slate.)\n\n${prompt}`
    }
    try {
      const result = await agent(attemptPrompt, opts)
      if (result !== null) return result
      log(`${opts.label}: attempt ${attempt}/${attempts} returned no result`)
    } catch (e) {
      const msg = `${(e && e.message) || e}`
      if (/abort/i.test(msg)) throw e
      log(`${opts.label}: attempt ${attempt}/${attempts} failed: ${msg}`)
    }
  }
  return null
}

const IMPL_REPORT = {
  type: 'object',
  required: ['unit', 'status', 'files_written', 'summary'],
  properties: {
    unit: { type: 'string' },
    status: { enum: ['translated', 'blocked'] },
    blocked_reason: { type: 'string' },
    files_written: { type: 'array', items: { type: 'string' }, description: 'absolute paths of every Rust file this unit wrote or changed' },
    tests_written: { type: 'array', items: { type: 'string' } },
    test_provenance: {
      type: 'array',
      items: { type: 'string' },
      description: 'per test: the Paper fixture/golden/JUnit source of its expected values, or "hand-written (no verification credit)"; "verification: none" when no Java-sourced expectation exists',
    },
    stubs_created: { type: 'array', items: { type: 'string' }, description: 'out-of-unit stub items created to satisfy cross-unit references' },
    commits: { type: 'array', items: { type: 'string' }, description: 'hashes of the commits this agent made' },
    open_questions: { type: 'array', items: { type: 'string' } },
    summary: { type: 'string' },
  },
}

const FINDINGS = {
  type: 'object',
  required: ['findings', 'cargo_check'],
  properties: {
    findings: {
      type: 'array',
      items: {
        type: 'object',
        required: ['file', 'description', 'severity'],
        properties: {
          file: { type: 'string' },
          line: { type: 'integer' },
          category: { type: 'string' },
          description: { type: 'string' },
          suggested_fix: { type: 'string' },
          severity: { enum: ['critical', 'major', 'minor'] },
        },
      },
    },
    cargo_check: {
      type: 'object',
      required: ['unit_errors', 'crate_errors', 'sample'],
      properties: {
        unit_errors: { type: 'integer', description: "errors whose primary span is in this unit's files (or a stub it created)" },
        crate_errors: { type: 'integer', description: 'all errors cargo check -p reported' },
        sample: { type: 'string', description: "the unit's own error messages, truncated; empty when unit_errors is 0" },
      },
    },
  },
}

const WAVE_CHECK = {
  type: 'object',
  required: ['crates', 'uncommitted'],
  properties: {
    crates: {
      type: 'array',
      items: {
        type: 'object',
        required: ['crate', 'pass', 'error_count'],
        properties: {
          crate: { type: 'string' },
          pass: { type: 'boolean' },
          error_count: { type: 'integer' },
          sample: { type: 'string' },
        },
      },
    },
    uncommitted: { type: 'array', items: { type: 'string' }, description: 'git status --porcelain lines (excluding PROGRESS.md)' },
  },
}

const CARGO = `PATH=$HOME/.cargo/bin:$PATH CARGO_BUILD_JOBS=4`

function location(u) {
  return `WORKTREE: \`${WT}\` (the lane branch is already checked out). Your shell does NOT start there: prefix
every shell command with \`cd ${WT} &&\` and use absolute paths under ${WT} for every file you read or write.
Rust code lives in ${WT}/crates; GOAL.md, PORTING.md, OWNERSHIP.md and CRATES.md are at ${WT}.
Java source of truth for unit "${u.id}" — read-only, exactly these ${u.javaFiles.length} file(s), nothing else of the package:
${u.javaFiles.map((f) => `  ${f}`).join('\n')}
Other Java under ${PAPER} may be read only to look up a dependency's signature. Never modify anything under working/.`
}

const COMMON_RULES = `HARD RULES (GOAL.md is binding; PORTING.md governs translation):
- Git: only the per-unit commit described below. NEVER git stash, reset, checkout, restore, rebase, merge, push,
  or force anything.
- Never modify MANIFEST.tsv, any *.md design doc, any fixture/golden/expected value/comparator, or files outside
  your unit's crate — except minimal STUB declarations for cross-unit types at their canonical home.
- Wrapping arithmetic for all Java int/long math; >>> is a logical shift; exact RNG and Mth tables; UTF-16-aware
  string indices where observable. See the PORTING.md drift checklist.
- No todo!()/unimplemented!() without status=blocked + blocked_reason. Never invent APIs or "improve" logic.
- Never copy from working/Pumpkin.
- Cargo: run every cargo command as \`cd ${WT} && ${CARGO} cargo <cmd> --locked ...\` (several lanes share this
  host). Run only \`cargo check -p <crate>\` and filtered tests (\`cargo nextest run -p <crate> -E 'test(<module>)'\`),
  never a workspace-wide test build, and never build under /tmp.`

const CANONICAL_TYPE_RULE = `CANONICAL TYPES: one Java class has exactly one Rust definition, at the module path mirroring its Java
package inside the crate CRATES.md assigns to that package. Before declaring any struct/enum/trait/type alias,
search for it (\`cd ${WT} && rg -n "(struct|enum|trait|type) <Name>\\b" crates tools\`); if it exists anywhere,
import and reuse it. Never define a second copy, a local stand-in, a simplified shadow, or a wrapper of an
existing type. Never define a hub type (a class many packages depend on: Entity, LivingEntity, Player,
ServerPlayer, Level, ServerLevel, MinecraftServer, BlockState, BlockPos, Vec3, AABB, ItemStack, ...) outside its
canonical home. If a needed type does not exist yet: a hub -> stop and return status=blocked naming the missing
hub (never stand it in); a leaf -> declare it minimally at its canonical path with a
\`// STUB(<owning-unit-id>) <reason>\` marker and list it in stubs_created.`

const TEST_RULE = `TESTS (D18): expected values come only from Paper — Java-generated fixtures/goldens already committed with
their manifests, or the values a ported Paper JUnit test asserts. Never compute an expected value by running or
reasoning through the Rust translation, and never write a test that pins the translation's own output (a snapshot
of its own output, or a Rust-vs-Rust round trip presented as parity): such tests enshrine translation bugs.
Hand-written tests (Rust-only invariants, error handling) are allowed but earn NO verification credit and must
not assert Paper behaviour from memory. Never edit an existing fixture, golden, expected value, or comparator.`

function commitRule(u) {
  return `COMMIT (per unit, on the lane branch): when your edits are done, commit ONLY the files you wrote or changed
for this unit, by explicit path:
  cd ${WT} && git add -- <paths> && git commit -m "port(${u.id}): <what changed>" -- <paths>
Other units of this wave are written concurrently in the same worktree: never \`git add -A\`, \`git add .\` or
\`git commit -a\`, and never commit, revert, or edit another unit's files. A shared file you had to touch (a
mod.rs line, a stub) is committed whole. If git reports an index.lock, wait a few seconds and retry. Do not push
— the controller pushes. Put the commit hashes in commits.`
}

function resumeNote(u) {
  return `This workflow gets paused and resumed: crates may already hold partial files for this unit from an
interrupted prior attempt. First run \`cd ${WT} && git status --short && git log --oneline -15\`, look for
port(${u.id}) commits and existing files, and continue from that state instead of starting over.`
}

const REVIEW_TEST_CHECKS = `Also check every test the unit added or changed (D18):
- For each literal expected value: where did it come from? Accept only a recorded Paper fixture/golden with a
  manifest, a value derived from one, a ported Paper JUnit assertion, or a Rust-only invariant with no Paper
  counterpart. Flag anything else as category fabricated-expectation (major).
- Any edit to a fixture, *.jsonl/*.tsv golden, exclusion table, or comparator is critical.
- A test that can pass vacuously, or compares Rivet output to Rivet output, earns no credit: say so.`

const LENSES = {
  'semantic-drift': `LENS — SEMANTIC DRIFT. Hunt ONLY translation bugs, using PORTING.md's drift checklist:
wrapping arithmetic omissions, >>> vs >>, HashMap iteration-order observability, UTF-16 index
drift, f32/f64 widening, Mth-table vs libm, dropped null checks, eager unwrap_or, identity-vs-
value equality, dropped synchronized without a note, debug_assert side effects, cast semantics,
inclusive/exclusive off-by-ones. Compare Rust line-by-line against the Java original.`,
  'ownership-api': `LENS — OWNERSHIP, API FIDELITY & CANONICAL TYPES. Verify the code follows OWNERSHIP.md (arenas + IDs, no
Arc/RwLock on game state, sync-tick assumptions), PORTING.md naming/module conventions, that no
Java logic or method was silently skipped or invented (diff the public surface against the listed Java
files), that stubs are minimal and marked, that every type is defined once at its canonical home (search the
workspace for duplicates, local stand-ins, or shadows of existing types — a duplicate hub type is critical),
and that tests were not weakened or fabricated.`,
  combined: `FULL LENS. Apply BOTH the PORTING.md drift checklist (wrapping arithmetic, >>> handling, map-order
observability, UTF-16 indices, float widening, Mth tables, null checks, equality semantics, cast semantics)
AND the architecture rules (OWNERSHIP.md arenas/IDs, no invented or skipped logic vs the Java original,
minimal marked stubs, every type defined once at its canonical home with no duplicates or stand-ins, tests
not weakened or fabricated).`,
}

function implementPrompt(u) {
  return `You are the IMPLEMENTER for Rivet port unit "${u.id}" (wave ${A.waveId}).
${location(u)}
${COMMON_RULES}
${CANONICAL_TYPE_RULE}
${TEST_RULE}
${commitRule(u)}

Unit: Java package ${u.java_package || '(see files)'}; target crate crates/${u.crate}, module path mirroring the
Java package per PORTING.md naming.
${u.notes ? `Unit notes: ${u.notes}` : ''}
${resumeNote(u)}

Steps:
1. Read GOAL.md, PORTING.md, and the relevant OWNERSHIP.md section for this crate.
2. Read every listed Java file fully; skim the public signatures of their direct dependencies.
3. Translate faithfully per PORTING.md into ${WT}/crates/${u.crate}/src/... — preserve structure, names,
   algorithms, constants. Reuse existing canonical types (rule above).
4. Port the Paper JUnit tests that cover these classes, if any, with their expected values unchanged. Add
   fixture-based tests only against Java-sourced goldens that already exist. Record each test's provenance in
   test_provenance; if no Java-sourced expectation exists, write "verification: none" and invent nothing.
5. Make \`cd ${WT} && ${CARGO} cargo check -p ${u.crate} --locked\` report zero errors in this unit's files.
   Errors located only in another unit's in-flight files are not yours: leave those files alone and mention
   the count in summary.
6. Commit per the COMMIT rule (also when reporting blocked, if you wrote files).

Your final output must be the structured report. files_written must be exhaustive and absolute.`
}

function reviewPrompt(u, report, lens, round) {
  return `You are a REVIEWER for Rivet port unit "${u.id}". You must NOT see or trust the implementer's reasoning —
judge only the code.${round > 1 ? ' The unit was already reviewed and fixed at least once; you are a FRESH verifier with no knowledge of prior findings — review the current state completely.' : ''}
${LENSES[lens]}
${CANONICAL_TYPE_RULE}
${REVIEW_TEST_CHECKS}

${location(u)}
Rust files the unit reports: ${JSON.stringify(report.files_written)}. Its commits are visible with
\`cd ${WT} && git log --oneline --grep "port(${u.id})"\`. Read the Java and the Rust fully.

You are READ-ONLY: do not edit files and do not run any git command that changes state. You MUST run
\`cd ${WT} && ${CARGO} cargo check -p ${u.crate} --locked --message-format=short\` and fill cargo_check:
unit_errors counts only errors whose primary span is in this unit's files (or a stub it created); other units
of the wave are written concurrently, so errors elsewhere count only in crate_errors.

Report every defect as a finding (severity: critical = wrong behavior/parity break, major =
convention/architecture violation or fabricated expectation, minor = style). An empty findings list is a
valid result — do not manufacture findings.`
}

function fixPrompt(u, report, findings) {
  return `You are the FIXER for Rivet port unit "${u.id}".
${location(u)}
${COMMON_RULES}
${CANONICAL_TYPE_RULE}
${TEST_RULE}
${commitRule(u)}
${resumeNote(u)}

Apply these reviewer findings to the unit's files ${JSON.stringify(report.files_written)}:
${JSON.stringify(findings, null, 1)}

Rules: fix all critical and major findings; apply minor ones when unambiguous. If a finding is WRONG
(reviewer misread the Java), do not apply it — record why in open_questions. Verify each fix against the
listed Java files. Re-run \`cd ${WT} && ${CARGO} cargo check -p ${u.crate} --locked\` until this unit's files
have zero errors, commit per the COMMIT rule, and return the updated structured report (files_written stays
exhaustive for the whole unit).`
}

async function reviewRound(u, report, round) {
  // Full rigor: round 1 gets both lenses (highest yield), later rounds one
  // fresh full-lens verifier. Lean rigor: one combined reviewer every round.
  const lenses = RIGOR === 'full' && round === 1 ? ['semantic-drift', 'ownership-api'] : ['combined']
  const results = await parallel(lenses.map((lens) => () =>
    resilientAgent(reviewPrompt(u, report, lens, round), {
      label: `rev:${u.id}:r${round}${lenses.length > 1 ? `:${lens}` : ''}`,
      phase: 'Review',
      schema: FINDINGS,
      ...REVIEW_OPTS,
    })))
  const ok = results.filter(Boolean)
  // All reviewers dead is an infrastructure failure, not a clean review — the
  // caller must not mistake it for zero findings.
  if (ok.length === 0) return null
  if (ok.length < results.length) log(`rev:${u.id}:r${round}: ${results.length - ok.length} reviewer(s) failed after retries`)
  const findings = ok.flatMap((r) => r.findings)
  // Convergence is gated on the build, not only on review: a unit whose own
  // files do not compile is never clean.
  const broken = ok.filter((r) => r.cargo_check.unit_errors > 0)
  if (broken.length > 0) {
    findings.unshift({
      file: '(build)',
      category: 'cargo-check',
      severity: 'critical',
      description: `cargo check -p ${u.crate} reports ${Math.max(...broken.map((r) => r.cargo_check.unit_errors))} error(s) in this unit's files:\n${broken[0].cargo_check.sample}`,
    })
  }
  return findings
}

function blockedResult(report, reason, round, totalFindings) {
  return { ...report, status: 'blocked', blocked_reason: reason, review_rounds: round, review_findings: totalFindings, converged: false }
}

async function converge(u, impl) {
  let report = impl
  let totalFindings = 0
  for (let round = 1; round <= MAX_REVIEW_ROUNDS; round++) {
    const findings = await reviewRound(u, report, round)
    if (findings === null) return blockedResult(report, `review round ${round}: all reviewers failed after retries`, round, totalFindings)
    if (findings.length === 0) {
      return { ...report, review_rounds: round, review_findings: totalFindings, converged: true }
    }
    totalFindings += findings.length
    const lastRound = round === MAX_REVIEW_ROUNDS
    const onlyMinor = findings.every((f) => f.severity === 'minor')
    if (lastRound && !onlyMinor) {
      return blockedResult(report, `did not converge: ${findings.length} findings after ${MAX_REVIEW_ROUNDS} review rounds (${RIGOR} rigor)`, round, totalFindings)
    }
    const fixed = await resilientAgent(fixPrompt(u, report, findings), {
      label: `fix:${u.id}:r${round}`, phase: 'Fix', schema: IMPL_REPORT, ...IMPL_OPTS,
    })
    if (!fixed) return blockedResult(report, `fix round ${round}: fixer failed after retries`, round, totalFindings)
    if (fixed.status === 'blocked') return { ...fixed, review_rounds: round, review_findings: totalFindings, converged: false }
    report = fixed
    if (lastRound) {
      // Minor-only tail: fixed but unverified — converged with a caveat, not blocked.
      return { ...report, review_rounds: round, review_findings: totalFindings, converged: 'with-unverified-minor-fixes' }
    }
  }
}

log(`wave ${A.waveId}: ${UNITS.length} units, ${RIGOR} rigor (max ${MAX_REVIEW_ROUNDS} review rounds), worktree ${WT}`)
for (const u of UNITS) {
  log(`${u.id} -> crates/${u.crate}: ${u.javaFiles.length} Java file(s)\n  ${u.javaFiles.join('\n  ')}`)
}

const results = await pipeline(
  UNITS,
  (u) => resilientAgent(implementPrompt(u), { label: `impl:${u.id}`, phase: 'Translate', schema: IMPL_REPORT, ...IMPL_OPTS }),
  (impl, u) => {
    if (!impl) {
      return { unit: u.id, status: 'blocked', blocked_reason: 'implementer failed after retries', files_written: [], summary: '' }
    }
    if (impl.status === 'blocked') return impl
    return converge(u, impl)
  },
)

const done = results.filter(Boolean)
const blocked = done.filter((r) => r.status === 'blocked')

// Wave-level check once every unit has settled: per-unit gates only count a
// unit's own errors (units of one crate are written concurrently), so the
// whole crate must also compile with every unit in place, and every unit must
// have committed its work.
phase('Check')
const crates = [...new Set(UNITS.map((u) => u.crate))]
const waveCheck = await resilientAgent(
  `Report the post-wave state of the lane worktree \`${WT}\`. Do not edit anything and run no git command that
changes state. For each crate in ${JSON.stringify(crates)} run
\`cd ${WT} && ${CARGO} cargo check -p <crate> --locked --message-format=short\` and report pass (zero errors),
error_count, and a truncated sample of the errors. Then run \`cd ${WT} && git status --porcelain\` and report every
line except PROGRESS.md in uncommitted.`,
  { label: `check:${A.waveId}`, phase: 'Check', schema: WAVE_CHECK, model: IMPL_OPTS.model, effort: 'low' },
)
const cratesClean = !!waveCheck && waveCheck.crates.length === crates.length && waveCheck.crates.every((c) => c.pass)
const committed = !!waveCheck && waveCheck.uncommitted.length === 0
if (!waveCheck) log(`wave ${A.waveId}: wave-level check failed after retries — crate state UNKNOWN`)
else {
  for (const c of waveCheck.crates) log(`cargo check -p ${c.crate}: ${c.pass ? 'clean' : `${c.error_count} error(s)`}`)
  if (!committed) log(`uncommitted leftovers: ${waveCheck.uncommitted.join(', ')}`)
}
log(`wave ${A.waveId} complete: ${done.length}/${UNITS.length} reported, ${blocked.length} blocked, crates ${cratesClean ? 'clean' : 'NOT clean'}`)
return { waveId: A.waveId, worktree: WT, rigor: RIGOR, results: done, crates_clean: cratesClean, all_committed: committed, wave_check: waveCheck }
