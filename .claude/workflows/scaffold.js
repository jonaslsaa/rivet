export const meta = {
  name: 'scaffold',
  description: "Scaffold one family's Rust surface (mirrored files, mod lines, type and fn signatures with STUB bodies) from the Java, then verify it with a fresh reviewer",
  whenToUse: 'Once per family, before its leaf translate-waves; the scaffold PR merges alone and first. Pass {worktree, family, javaPaths, crate, modulePath, implModel?, implEffort?}. worktree is the absolute lane worktree path (pinned in every prompt because workflow agents inherit the session cwd); javaPaths are MANIFEST.tsv root:relpath entries; modulePath is the Rust module the family mirrors inside crates/<crate>/src (e.g. world::level::block). One agent (implModel/implEffort, default sonnet/high) writes the surface and commits; one fresh reviewer per round checks it against the Java and cargo check -p <crate>; at most 2 rounds.',
  phases: [{ title: 'Scaffold' }, { title: 'Review' }, { title: 'Fix' }],
}

// args sometimes arrives JSON-encoded as a string instead of an object.
const A = typeof args === 'string' ? JSON.parse(args) : args
if (!A || !A.worktree || !A.family || !A.crate || !A.modulePath || !Array.isArray(A.javaPaths) || A.javaPaths.length === 0) {
  throw new Error('scaffold requires {worktree, family, javaPaths: [...], crate, modulePath}')
}
// Workflow agents inherit the SESSION cwd, not the lane worktree.
const WT = String(A.worktree).replace(/\/+$/, '')
if (!WT.startsWith('/')) throw new Error(`scaffold: worktree must be an absolute path (got ${WT})`)
const WT_MARK = '/.claude/worktrees/'
const MAIN = WT.includes(WT_MARK) ? WT.slice(0, WT.indexOf(WT_MARK)) : WT
const PAPER = `${MAIN}/working/Paper`
// MANIFEST.tsv java_paths roots (scripts/analyze_graph.py SOURCE_ROOTS).
const SOURCE_ROOTS = {
  minecraft: `${PAPER}/paper-server/src/minecraft/java`,
  'paper-server': `${PAPER}/paper-server/src/main/java`,
  'paper-api': `${PAPER}/paper-api/src/main/java`,
}
const MAX_ROUNDS = 2
const IMPL_OPTS = { model: A.implModel || 'sonnet', effort: A.implEffort || 'high' }
const CARGO = 'PATH=$HOME/.cargo/bin:$PATH CARGO_BUILD_JOBS=4'

const ENTRIES = A.javaPaths.map((e) => String(e).trim()).filter(Boolean)
const FILES = ENTRIES.map((e) => {
  const i = e.indexOf(':')
  const root = i > 0 ? e.slice(0, i) : ''
  const rel = i > 0 ? e.slice(i + 1) : ''
  if (!SOURCE_ROOTS[root] || !rel.endsWith('.java')) {
    throw new Error(`scaffold: malformed javaPaths entry "${e}" (want root:relpath, root one of ${Object.keys(SOURCE_ROOTS).join('/')})`)
  }
  return { entry: e, path: `${SOURCE_ROOTS[root]}/${rel}` }
})

// A stalled provider call must cost one retry, not the whole run. One extra
// attempt only — the harness already retries internally. Never retry a user
// abort. The retry preamble makes the attempt a distinct journal key.
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
  required: ['status', 'files_written', 'summary'],
  properties: {
    status: { enum: ['scaffolded', 'blocked'] },
    blocked_reason: { type: 'string' },
    files_written: { type: 'array', items: { type: 'string' }, description: 'absolute paths of every file written or changed' },
    stub_owners: { type: 'array', items: { type: 'string' }, description: '"<root:relpath> -> <manifest unit id>" for every Java file' },
    stubs_created: { type: 'array', items: { type: 'string' }, description: 'out-of-family stub types declared at their canonical home' },
    commits: { type: 'array', items: { type: 'string' } },
    disputed: { type: 'array', items: { type: 'string' }, description: 'findings not applied because they are factually wrong, with why' },
    summary: { type: 'string' },
  },
}

const VERDICT = {
  type: 'object',
  required: ['findings', 'cargo_check_pass', 'files_inspected'],
  properties: {
    findings: {
      type: 'array',
      items: {
        type: 'object',
        required: ['file', 'description', 'severity'],
        properties: {
          file: { type: 'string' },
          line: { type: 'integer' },
          description: { type: 'string' },
          severity: { enum: ['critical', 'major', 'minor'] },
        },
      },
    },
    cargo_check_pass: { type: 'boolean' },
    cargo_check_output: { type: 'string', description: 'first ~40 error lines when it fails' },
    files_inspected: { type: 'array', items: { type: 'string' } },
    summary: { type: 'string' },
  },
}

const WHERE = `WORKTREE: \`${WT}\` (the lane branch is already checked out). Your shell does NOT start there: prefix
every shell command with \`cd ${WT} &&\` and use absolute paths under ${WT} for every file you read or write.
Design docs (GOAL.md, PORTING.md, OWNERSHIP.md, CRATES.md, WORKFLOWS.md) are at ${WT}.
Family "${A.family}": target crate crates/${A.crate}, Rust module \`${A.modulePath}\` inside ${WT}/crates/${A.crate}/src.
Java source of truth (read-only; never modify anything under working/) — exactly these ${FILES.length} file(s):
${FILES.map((f) => `  ${f.entry}  =>  ${f.path}`).join('\n')}`

const SURFACE_RULES = `WHAT A SCAFFOLD IS: the family's compile-clean Rust SURFACE, no logic. For every listed Java file:
- One Rust file at the mirrored path per PORTING.md naming (class -> snake_case module under ${A.modulePath};
  nested/inner classes per PORTING.md), plus the \`mod\` lines (and only those) needed to reach it from the
  crate root. Do not reorder or rewrite unrelated mod lines.
- Every non-private Java type and member: classes/records/interfaces -> struct/trait per PORTING.md and
  OWNERSHIP.md, enums with all constants, public/protected/package-private methods and constructors as fn
  signatures (overloads named per PORTING.md), constants as consts when their value is a literal. Visibility:
  public -> pub, protected/package-private -> pub(crate). Private members are left to the unit's port.
- Fields only where their types already exist canonically; otherwise leave the field list to the unit and say
  so in the stub reason.
- Every fn body is a stub: a \`// STUB(<unit-id>) <what is absent>\` comment line immediately before
  \`panic!("STUB(<unit-id>): <JavaClass>#<method> not ported")\`, with \`let _ = (<params>);\` first when the fn has
  parameters. Never todo!()/unimplemented!() (scripts/check_markers.py rejects them without a RivetTodo).
  <unit-id> is the MANIFEST.tsv unit whose java_paths cell contains that file's root:relpath entry
  (\`cd ${WT} && grep -F '<root:relpath>' MANIFEST.tsv\`); it must exist and not be done. If a file has no
  owning unit, return status=blocked.
- CANONICAL TYPES: one Java class has exactly one Rust definition. Before declaring any type, search for it
  (\`cd ${WT} && rg -n "(struct|enum|trait|type) <Name>\\b" crates tools\`) and reuse it if it exists anywhere.
  A signature that needs a missing hub type (Entity, Level, ServerLevel, BlockState, BlockPos, Vec3, AABB,
  ItemStack, ...) -> status=blocked naming it; a missing leaf type outside the family -> a minimal declaration
  at its canonical home with its owning unit's STUB marker, listed in stubs_created.
- The scaffold must build warning-free: \`cd ${WT} && ${CARGO} cargo check -p ${A.crate} --locked\` with zero
  errors and \`cd ${WT} && RUSTFLAGS=-Dwarnings ${CARGO} cargo clippy -p ${A.crate} --all-targets --locked\` clean.
  A pub(crate) item that trips dead_code gets an item-level \`#[allow(dead_code)]\` preceded, on its own line directly above it,
  by \`// STUB(<unit-id>) scaffold\` (a trailing comment is not a marker, so check_markers.py would never flag it stale),
  never a module-wide allow. \`cd ${WT} && python3 scripts/check_markers.py\` must pass.
- No tests, no fixtures, no MANIFEST.tsv or *.md edits, nothing under working/, no copying from working/Pumpkin.`

const COMMIT_RULE = `Commit on the lane branch with explicit paths:
\`cd ${WT} && git add -- <paths> && git commit -m "scaffold(${A.family}): <what>" -- <paths>\`. NEVER git stash, reset,
checkout, rebase, merge, push, or force anything — the controller pushes. Put the commit hashes in commits.
Use PATH=$HOME/.cargo/bin:$PATH and CARGO_BUILD_JOBS=4 for cargo; never build under /tmp.
This workflow gets paused and resumed: first run \`cd ${WT} && git status --short && git log --oneline -10\` and
continue from any partial scaffold already there instead of starting over.`

function scaffoldPrompt() {
  return `You are the SCAFFOLD AUTHOR for Rivet family "${A.family}".
${WHERE}

${SURFACE_RULES}

${COMMIT_RULE}

Read PORTING.md (naming), OWNERSHIP.md (type shapes), and the WORKFLOWS.md marker rules first, then every listed
Java file. Return the structured report; files_written must be exhaustive and absolute, and stub_owners must
map every listed Java file to its unit id.`
}

function reviewPrompt(report, round) {
  return `You are a FRESH REVIEWER of the Rivet scaffold for family "${A.family}"${round > 1 ? ' after one fix round; review the current state completely' : ''}.
Do not trust the author's reasoning — judge only the code against the Java.
${WHERE}

The scaffold must satisfy these rules:
${SURFACE_RULES}

Inspect: \`cd ${WT} && git log --oneline --grep "scaffold(${A.family})"\`, the files the author reports
(${JSON.stringify(report.files_written)}), and \`cd ${WT} && git status --short\` (uncommitted scaffold files are
a finding). For every listed Java file check that the Rust surface MATCHES it: each non-private type and member
present with the PORTING.md name, parameter and return types mapped per PORTING.md, static vs instance kept,
visibility mapped as above, enum constants complete and in order, nothing invented and no logic ported, files and
mod lines mirroring the Java package, every STUB marker naming the right owning unit, no duplicate or stand-in of
an existing canonical type. Run \`cd ${WT} && ${CARGO} cargo check -p ${A.crate} --locked\` (set cargo_check_pass),
the clippy command, and \`cd ${WT} && python3 scripts/check_markers.py\`; a failure of any is a critical finding.

You are READ-ONLY: do not edit files and run no git command that changes state. Severity: critical = surface
mismatch with the Java or a broken build, major = convention violation, minor = style. An empty findings list is
a valid result — do not manufacture findings. List every file you read in files_inspected.`
}

function fixPrompt(report, findings) {
  return `You are the SCAFFOLD AUTHOR for Rivet family "${A.family}", applying reviewer findings.
${WHERE}

${SURFACE_RULES}

${COMMIT_RULE}

Findings:
${JSON.stringify(findings, null, 1)}

Fix every critical and major finding and unambiguous minor ones. If a finding is factually WRONG (the reviewer
misread the Java), do not apply it — list it in disputed with why. Return the updated structured report
(files_written stays exhaustive for the whole scaffold).`
}

log(`scaffold ${A.family}: ${FILES.length} Java file(s) -> crates/${A.crate} ${A.modulePath}, worktree ${WT}`)
log(FILES.map((f) => `  ${f.path}`).join('\n'))

phase('Scaffold')
let report = await resilientAgent(scaffoldPrompt(), { label: `scaffold:${A.family}`, phase: 'Scaffold', schema: IMPL_REPORT, ...IMPL_OPTS })
if (!report) return { family: A.family, converged: false, blocked_reason: 'scaffold author failed after retries', findings: [] }
if (report.status === 'blocked') return { family: A.family, converged: false, blocked_reason: report.blocked_reason, report, findings: [] }

let all = []
for (let round = 1; round <= MAX_ROUNDS; round++) {
  phase('Review')
  const verdict = await resilientAgent(reviewPrompt(report, round), { label: `rev:${A.family}:r${round}`, phase: 'Review', schema: VERDICT, effort: 'high' })
  if (!verdict) return { family: A.family, converged: false, blocked_reason: `review round ${round}: reviewer failed after retries`, report, findings: all, rounds: round }
  const findings = verdict.findings.slice()
  if (!verdict.cargo_check_pass) {
    findings.unshift({ file: '(build)', severity: 'critical', description: `cargo check -p ${A.crate} failed:\n${verdict.cargo_check_output || '(no output captured)'}` })
  }
  all = all.concat(findings.map((f) => ({ ...f, round })))
  if (findings.length === 0) {
    log(`scaffold ${A.family}: clean after ${round} review round(s)`)
    return { family: A.family, converged: true, report, findings: all, rounds: round }
  }
  if (round === MAX_ROUNDS) break
  phase('Fix')
  const fixed = await resilientAgent(fixPrompt(report, findings), { label: `fix:${A.family}:r${round}`, phase: 'Fix', schema: IMPL_REPORT, ...IMPL_OPTS })
  if (!fixed) return { family: A.family, converged: false, blocked_reason: `fix round ${round}: author failed after retries`, report, findings: all, rounds: round }
  if (fixed.status === 'blocked') return { family: A.family, converged: false, blocked_reason: fixed.blocked_reason, report: fixed, findings: all, rounds: round }
  report = fixed
}
log(`scaffold ${A.family}: not converged after ${MAX_ROUNDS} review rounds — controller triage`)
return { family: A.family, converged: false, report, findings: all, rounds: MAX_ROUNDS }
