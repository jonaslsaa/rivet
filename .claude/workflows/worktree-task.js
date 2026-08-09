export const meta = {
  name: 'worktree-task',
  description: 'Implement a scoped non-translate-wave task in an isolated worktree (implementer self-reviews via nested subagents), then gate with independent fresh reviewers until clean',
  whenToUse: 'Substantive tasks that are NOT manifest-unit translations: tooling, harnesses, codegen, infra scripts. Pass {worktree, task, maxRounds?, fallbackModel?}. The implementer works in the named worktree (path pinned in every prompt because workflow agents inherit the session cwd) and runs its own inner review-fix loop with nested subagents; independent script-level gate reviewers decide convergence, never the implementer.',
  phases: [{ title: 'Implement' }, { title: 'Gate' }, { title: 'Fix' }],
}

const MAX = (args && args.maxRounds) || 6
if (!args || !args.worktree || !args.task) throw new Error('worktree-task requires {worktree, task}')
const WT = args.worktree
const SLUG = WT.split('/').pop()
const FALLBACK_MODEL = args && args.fallbackModel

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
  },
}

// A stalled provider call or mid-stream disconnect must cost one retry, not the
// whole run: agent() throws on harness-level stall and returns null on terminal
// API errors, and historically either one killed a multi-hour workflow. The
// attempt preamble also makes each retry a distinct journal key, so a resumed
// run never replays a half-dead attempt as if it had completed.
async function resilientAgent(prompt, opts, attempts = 3) {
  for (let attempt = 1; attempt <= attempts; attempt++) {
    const attemptOpts = { ...opts }
    let attemptPrompt = prompt
    if (attempt > 1) {
      attemptPrompt = `(Retry ${attempt}/${attempts}: the previous attempt died mid-run from a provider error. Its partial file changes may still be on disk — reconcile, don't assume a clean slate.)\n\n${prompt}`
      if (FALLBACK_MODEL && attempt === attempts) attemptOpts.model = FALLBACK_MODEL
    }
    try {
      const result = await agent(attemptPrompt, attemptOpts)
      if (result !== null) return result
      log(`${opts.label}: attempt ${attempt}/${attempts} returned no result`)
    } catch (e) {
      log(`${opts.label}: attempt ${attempt}/${attempts} failed: ${e && e.message}`)
    }
  }
  return null
}

const RESUME_NOTE =
  `The worktree may already contain partial work from a prior interrupted attempt (this workflow ` +
  `gets paused and resumed). FIRST run \`git status\` in \`${WT}\` and read \`${WT}/PROGRESS.md\` if it ` +
  `exists; continue from that state instead of starting over. Maintain PROGRESS.md as you work: ` +
  `what is done, what is in flight, what remains (overwrite, keep it short). Delete PROGRESS.md ` +
  `as your final action before reporting.`

const UNCOMMITTED_NOTE =
  `The work is intentionally UNCOMMITTED — implementers are forbidden to run git commit (the ` +
  `controller commits). Judge the working tree: \`git status\` + \`git diff origin/main\` in the ` +
  `worktree. Zero commits over origin/main is expected and is NOT a finding.`

phase('Implement')
const impl = await resilientAgent(
  `${args.task}\n\nWork in the git worktree at \`${WT}\` (its branch is already checked out). ` +
    `Java source of truth is at \`${WT}/working\` (read-only symlink — never modify or commit from it). ` +
    `Do NOT run git commit/stash/reset. Use PATH=\`$HOME/.cargo/bin:$PATH\` for cargo. ` +
    `${RESUME_NOTE}\n\n` +
    `When you believe the task is complete, run your own inner review loop (at most 2 rounds) before ` +
    `reporting: spawn a FRESH reviewer subagent with the Agent tool (run_in_background: false), giving ` +
    `it ONLY the task text and the worktree path — none of your reasoning — and telling it to hunt ` +
    `correctness, fidelity to Paper, and process violations in the working tree. Apply its critical and ` +
    `major findings, then (if there were any) spawn one more fresh reviewer to re-check. Include each ` +
    `reviewer's verdict VERBATIM in your report — do not summarize them away.\n\n` +
    `Report exactly what you built, the files you produced, how to run it, the verbatim inner-review ` +
    `verdicts, and any blockers.`,
  { label: `impl:${SLUG}`, phase: 'Implement' },
)

let all = []
let prior = null
for (let r = 1; r <= MAX; r++) {
  phase(r === 1 ? 'Gate' : `Gate r${r}`)
  const verdict = await resilientAgent(
    `${r === 1
      ? 'You are a FRESH adversarial reviewer of a newly built deliverable. The implementer already ran its own review loop — do not trust that; it is not your input.'
      : `You are a FRESH full-lens verifier. Review the CURRENT state completely, and additionally verify that each previous-round finding below was actually fixed (a finding merely disputed by the fixer counts as fixed ONLY if the fixer's stated reason holds up against the code):\n${JSON.stringify(prior, null, 2)}`}\n\n` +
      `Task: ${args.task}\n` +
      `Inspect the work in \`${WT}\`. ${UNCOMMITTED_NOTE} Check against the task, the project docs ` +
      `(GOAL.md/PORTING.md/OWNERSHIP.md in the worktree), and the Java source in \`${WT}/working\` where relevant. ` +
      `Hunt correctness, fidelity to Paper, process violations (weakened tests, invented APIs, missing STUB markers ` +
      `for deliberately omitted parts, non-committed scaffold drift). Set merge_ready=false if any critical or major ` +
      `finding exists. Do NOT manufacture findings — an empty list with merge_ready=true is a valid result.`,
    { label: `gate:${SLUG}:r${r}`, phase: 'Gate', schema: VERDICT, effort: 'high' },
  )
  if (!verdict) {
    log(`gate r${r} failed after retries — stopping for controller triage`)
    break
  }
  all = all.concat(verdict.findings.map((f) => ({ ...f, round: r })))
  if (verdict.merge_ready) {
    log(`converged after ${r} gate round(s)`)
    return { merge_ready: true, findings: all, worktree: WT }
  }
  prior = verdict.findings
  if (r < MAX) {
    await resilientAgent(
      `Apply the reviewer findings in worktree \`${WT}\`. Findings:\n${JSON.stringify(verdict.findings, null, 2)}\n\n` +
        `${RESUME_NOTE}\n\n` +
        `Fix correctness/fidelity/process issues; do NOT weaken tests or invent shortcuts; mark any deliberately ` +
        `omitted part // STUB with a reason. If a finding is WRONG, do not apply it — record exactly why in your ` +
        `report. Do NOT commit. After fixing, spawn one FRESH reviewer subagent with the Agent tool ` +
        `(run_in_background: false) to re-check just these findings against the working tree, and include its ` +
        `verdict verbatim. Report what you changed and re-run the deliverable's check (cargo build/test where ` +
        `applicable) with PATH=\`$HOME/.cargo/bin:$PATH\`.`,
      { label: `fix:${SLUG}:r${r}`, phase: 'Fix' },
    )
  }
}
log(`${all.length} findings after ${MAX} rounds — not converged (controller triage)`)
return { merge_ready: false, findings: all, worktree: WT }
