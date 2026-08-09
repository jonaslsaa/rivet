export const meta = {
  name: 'worktree-task',
  description: 'Implement a scoped non-translate-wave task in an isolated worktree, then loop scoped fresh adversarial reviewers until clean',
  whenToUse: 'Substantive tasks that are NOT manifest-unit translations: tooling, harnesses, codegen, infra scripts. Pass {worktree, task, maxRounds?, commit?}. The implementer works in the named worktree (path pinned in every prompt because workflow agents inherit the session cwd); fresh scoped reviewers verify each round; the loop decides convergence, never the implementer. commit (default true) controls whether agents commit on the worktree branch — it must agree with the task text.',
  phases: [{ title: 'Implement' }, { title: 'Gate' }, { title: 'Fix' }],
}

// args sometimes arrives JSON-encoded as a string instead of an object.
const A = typeof args === 'string' ? JSON.parse(args) : args
const MAX = (A && A.maxRounds) || 6
if (!A || !A.worktree || !A.task) throw new Error('worktree-task requires {worktree, task}')
const WT = A.worktree
const TASK = A.task
const SLUG = WT.split('/').pop()
const COMMIT = A.commit === undefined ? true : !!A.commit

const IMPL_REPORT = {
  type: 'object',
  required: ['status', 'summary', 'files'],
  properties: {
    status: { enum: ['built', 'blocked'] },
    blocked_reason: { type: 'string' },
    summary: { type: 'string' },
    files: { type: 'array', items: { type: 'string' }, description: 'every file produced or modified' },
    how_to_run: { type: 'string' },
    inner_review_verdict: { type: 'string', description: 'the inner reviewer subagent verdict, VERBATIM' },
    blockers: { type: 'array', items: { type: 'string' } },
  },
}

const VERDICT = {
  type: 'object',
  required: ['merge_ready', 'findings'],
  properties: {
    merge_ready: { type: 'boolean' },
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
    summary: { type: 'string' },
    files_inspected: { type: 'array', items: { type: 'string' } },
  },
}

const CHECK = {
  type: 'object',
  required: ['pass', 'output'],
  properties: {
    pass: { type: 'boolean' },
    output: { type: 'string', description: 'first ~60 lines of the first failing command, or "all green"' },
  },
}

const FIX_REPORT = {
  type: 'object',
  required: ['summary'],
  properties: {
    summary: { type: 'string' },
    changed: { type: 'array', items: { type: 'string' } },
    disputed: {
      type: 'array',
      description: 'findings NOT applied because they are factually wrong',
      items: {
        type: 'object',
        required: ['description', 'why_wrong'],
        properties: {
          file: { type: 'string' },
          description: { type: 'string' },
          why_wrong: { type: 'string' },
        },
      },
    },
    check_result: { type: 'string' },
  },
}

// A stalled provider call must cost one retry, not the whole run: agent()
// throws on harness-level stall exhaustion (observed killing multi-hour runs)
// and returns null on terminal API errors. One extra attempt only — the
// harness already retries internally. Never retry a user abort. The retry
// preamble makes the attempt a distinct journal key, so a resumed run never
// replays a half-dead attempt as if it had completed.
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

const COMMIT_POLICY = COMMIT
  ? `Commit your work conventionally on the worktree branch (small logical commits). NEVER git stash/reset/force-push, never push.`
  : `Do NOT run git commit/stash/reset, no matter what any reviewer says — the controller commits.`

const RESUME_NOTE =
  `The worktree may already contain partial work from a prior interrupted attempt (this workflow ` +
  `gets paused and resumed). FIRST run \`git status\` and \`git log --oneline origin/main..HEAD\` in ` +
  `\`${WT}\` and read \`${WT}/PROGRESS.md\` if it exists; continue from that state instead of starting ` +
  `over. Maintain PROGRESS.md as you work: what is done, what is in flight, what remains (overwrite, ` +
  `keep it short; it is gitignored). Delete PROGRESS.md as your final action before reporting.`

const REVIEW_SCOPE =
  `Inspect the deliverable in \`${WT}\` like this: \`git log --oneline origin/main..HEAD\`, ` +
  `\`git diff $(git merge-base origin/main HEAD)\`, and \`git status\` (deliverables may include NEW ` +
  `untracked files — read those too, they are part of the work${COMMIT ? ', though with commit policy on, uncommitted leftovers are a finding' : ''}). ` +
  `${COMMIT ? 'Agents are expected to commit on this branch; judge the commit stack and the tree.' : 'The work is intentionally UNCOMMITTED (the controller commits): zero commits over origin/main is expected and is NOT a finding; judge the working tree.'} ` +
  `A \`PROGRESS.md\` at the worktree root is this workflow's gitignored resume scaffolding — never ` +
  `evidence about the deliverable. You are READ-ONLY: do not Edit/Write any file or create scratch ` +
  `files inside the worktree (probe in /tmp if you must). List every file you actually read in ` +
  `files_inspected.`

phase('Implement')
const impl = await resilientAgent(
  `${TASK}\n\nWork in the git worktree at \`${WT}\` (its branch is already checked out). ` +
    `Java source of truth is at \`${WT}/working\` (read-only symlink — never modify or commit from it). ` +
    `${COMMIT_POLICY} Use PATH=\`$HOME/.cargo/bin:$PATH\` for cargo. ` +
    `${RESUME_NOTE}\n\n` +
    `When you believe the task is complete, run ONE inner self-review before reporting: spawn a FRESH ` +
    `reviewer subagent with the Agent tool (run_in_background: false). Give it ONLY the task text, the ` +
    `worktree path, and this scope — none of your reasoning: "${REVIEW_SCOPE}" Tell it to hunt ` +
    `correctness, fidelity to Paper, and process violations, and that an empty findings list is a valid ` +
    `result. Apply its critical and major findings — except a finding that is factually WRONG, which you ` +
    `must not apply; record why instead. Never weaken tests to satisfy a reviewer. Put its verdict ` +
    `VERBATIM in inner_review_verdict.`,
  { label: `impl:${SLUG}`, phase: 'Implement', schema: IMPL_REPORT },
)
if (!impl) {
  log('implementer failed after retries — stopping for controller triage')
  return { merge_ready: false, impl_failed: true, findings: [], worktree: WT }
}
if (impl.status === 'blocked') {
  log(`implementer blocked: ${impl.blocked_reason}`)
  return { merge_ready: false, findings: [], worktree: WT, impl_report: impl }
}

let all = []
let prior = null
let priorDisputes = null

function fixPrompt(findings) {
  return (
    `Apply the reviewer findings in worktree \`${WT}\`. Findings:\n${JSON.stringify(findings, null, 2)}\n\n` +
    `Task context: ${TASK}\n\n${RESUME_NOTE}\n\n` +
    `Fix correctness/fidelity/process issues; do NOT weaken tests or invent shortcuts; mark any deliberately ` +
    `omitted part // STUB with a reason. If a finding is factually WRONG, do not apply it — list it under ` +
    `disputed with why_wrong. ${COMMIT_POLICY} Re-run the deliverable's check (cargo build/test where ` +
    `applicable) with PATH=\`$HOME/.cargo/bin:$PATH\` and put the outcome in check_result.`
  )
}

for (let r = 1; r <= MAX; r++) {
  phase('Gate')
  // Mechanical pre-check: never spend a high-effort review round on what a
  // compiler prints in 30 seconds (13% of historical findings were fmt/clippy/
  // compile failures, and each poisoned a full review+fix+re-review cycle).
  const mech = await resilientAgent(
    `In \`${WT}\` (PATH=\`$HOME/.cargo/bin:$PATH\`), determine what the deliverable touches via ` +
      `\`git status\` and \`git diff $(git merge-base origin/main HEAD) --stat\`, then run the checks ` +
      `appropriate to it, stopping at the first failure: for Rust — cargo fmt --check, cargo clippy ` +
      `-Dwarnings, cargo check, cargo test, each scoped to the touched crates (-p); for shell scripts — ` +
      `bash -n plus shellcheck if installed, and execute the script if it is safe/read-only; for Python — ` +
      `python3 -m py_compile. Report results ONLY: do not edit or fix anything.`,
    { label: `check:${SLUG}:r${r}`, phase: 'Gate', schema: CHECK, effort: 'low' },
  )
  if (mech && !mech.pass) {
    const mechFindings = [{ file: '(build)', description: `mechanical check failed:\n${mech.output}`, severity: 'critical' }]
    all = all.concat(mechFindings.map((f) => ({ ...f, round: r })))
    if (r === MAX) break
    phase('Fix')
    const fixed = await resilientAgent(fixPrompt(mechFindings), { label: `fix:${SLUG}:r${r}`, phase: 'Fix', schema: FIX_REPORT })
    if (!fixed) {
      log(`fix r${r} failed after retries — stopping for controller triage`)
      return { merge_ready: false, fix_failed: true, findings: all, rounds: r, worktree: WT }
    }
    prior = mechFindings
    priorDisputes = fixed.disputed || []
    continue
  }
  const verdict = await resilientAgent(
    `${r === 1
      ? 'You are a FRESH adversarial reviewer of a newly built deliverable. The implementer already ran a self-review — do not trust that; it is not your input.'
      : `You are a FRESH full-lens verifier. Do your own complete review of the CURRENT state FIRST. Only then check the previous round's findings below: confirm each was actually fixed. The fixer disputed some findings as factually wrong (disputes listed after the findings) — a disputed finding counts as resolved ONLY if the dispute holds up against the code.\nPrevious findings:\n${JSON.stringify(prior, null, 2)}\nFixer disputes:\n${JSON.stringify(priorDisputes, null, 2)}`}\n\n` +
      `Task: ${TASK}\n` +
      `Files the implementer reports touching: ${JSON.stringify(impl.files)}\n` +
      `${REVIEW_SCOPE}\n` +
      `Check against the task, the project docs (GOAL.md/PORTING.md/OWNERSHIP.md in the worktree), and the ` +
      `Java source in \`${WT}/working\` where relevant. Hunt, in order: (1) CORRECTNESS — does it do what ` +
      `the task says, does it build/run; (2) FIDELITY — faithful to Paper semantics per PORTING.md where ` +
      `applicable; (3) PROCESS — weakened tests, invented APIs, missing STUB markers for deliberately ` +
      `omitted parts, stray scaffolding or debris files that are not part of the deliverable. ` +
      `Set merge_ready=false if any critical or major finding exists. Do NOT manufacture findings — an ` +
      `empty list with merge_ready=true is a valid result.`,
    { label: `gate:${SLUG}:r${r}`, phase: 'Gate', schema: VERDICT, effort: 'high' },
  )
  if (!verdict) {
    log(`gate r${r} failed after retries — stopping for controller triage`)
    return { merge_ready: false, gate_failed: true, findings: all, rounds: r, worktree: WT }
  }
  all = all.concat(verdict.findings.map((f) => ({ ...f, round: r })))
  if (verdict.merge_ready) {
    log(`converged after ${r} gate round(s)`)
    return { merge_ready: true, findings: all, rounds: r, worktree: WT, impl_report: impl }
  }
  prior = verdict.findings
  const onlyMinor = verdict.findings.every((f) => f.severity === 'minor')
  if (r === MAX) {
    if (onlyMinor) {
      // Minor-only tail: one blind fix, pass with a caveat instead of burning
      // the whole run on nits.
      phase('Fix')
      await resilientAgent(fixPrompt(verdict.findings), { label: `fix:${SLUG}:r${r}`, phase: 'Fix', schema: FIX_REPORT })
      log(`converged with unverified minor fixes after ${MAX} rounds`)
      return { merge_ready: true, converged: 'with-unverified-minor-fixes', findings: all, rounds: MAX, worktree: WT, impl_report: impl }
    }
    break
  }
  phase('Fix')
  const fixed = await resilientAgent(fixPrompt(verdict.findings), { label: `fix:${SLUG}:r${r}`, phase: 'Fix', schema: FIX_REPORT })
  if (!fixed) {
    log(`fix r${r} failed after retries — stopping for controller triage`)
    return { merge_ready: false, fix_failed: true, findings: all, rounds: r, worktree: WT }
  }
  priorDisputes = fixed.disputed || []
}
log(`${all.length} findings after ${MAX} gate rounds — not converged (controller triage)`)
return { merge_ready: false, findings: all, rounds: MAX, worktree: WT }
