export const meta = {
  name: 'worktree-task',
  description: 'Implement a scoped non-translate-wave task in an isolated worktree, then loop fresh adversarial reviewers until clean',
  whenToUse: 'Substantive tasks that are NOT manifest-unit translations: tooling, harnesses, codegen, infra scripts. Pass {worktree, task, maxRounds?}. The implementer works in the named worktree (path pinned in every prompt because workflow agents inherit the session cwd); fresh reviewers verify each round; the loop decides convergence, never the implementer.',
  phases: [{ title: 'Implement' }, { title: 'Review' }, { title: 'Fix' }],
}

const MAX = (args && args.maxRounds) || 3
if (!args || !args.worktree || !args.task) throw new Error('worktree-task requires {worktree, task}')
const WT = args.worktree
const SLUG = WT.split('/').pop()

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

phase('Implement')
const impl = await agent(
  `${args.task}\n\nWork in the git worktree at \`${WT}\` (its branch is already checked out). ` +
    `Java source of truth is at \`${WT}/working\` (read-only symlink — never modify or commit from it). ` +
    `Do NOT run git commit/stash/reset. Use PATH=\`$HOME/.cargo/bin:$PATH\` for cargo. ` +
    `Report exactly what you built, the files you produced, how to run it, and any blockers.`,
  { label: `impl:${SLUG}`, phase: 'Implement' },
)

let all = []
for (let r = 1; r <= MAX; r++) {
  phase(r === 1 ? 'Review' : `Fix r${r}`)
  const verdict = await agent(
    `${r === 1
      ? 'You are a FRESH adversarial reviewer of a newly built deliverable.'
      : 'You are a FRESH full-lens verifier, blind to prior findings. Review the CURRENT state completely; verify the previous round\'s findings were actually fixed.'}\n\n` +
      `Task: ${args.task}\n` +
      `Inspect the work in \`${WT}\` (git status + the produced files). Check against the task, the project docs ` +
      `(GOAL.md/PORTING.md/OWNERSHIP.md in the worktree), and the Java source in \`${WT}/working\` where relevant. ` +
      `Hunt correctness, fidelity to Paper, process violations (weakened tests, invented APIs, missing STUB markers ` +
      `for deliberately omitted parts, non-committed scaffold drift). Set merge_ready=false if any critical or major ` +
      `finding exists. Do NOT manufacture findings — an empty list with merge_ready=true is a valid result.`,
    { label: `rev:${SLUG}:r${r}`, phase: r === 1 ? 'Review' : 'Fix', schema: VERDICT, effort: 'high' },
  )
  if (!verdict) {
    log(`review r${r} failed`)
    break
  }
  all = all.concat(verdict.findings.map((f) => ({ ...f, round: r })))
  if (verdict.merge_ready) {
    log(`converged after ${r} round(s)`)
    return { merge_ready: true, findings: all, worktree: WT }
  }
  if (r < MAX) {
    const fixed = await agent(
      `Apply the reviewer findings in worktree \`${WT}\`. Findings:\n${JSON.stringify(verdict.findings, null, 2)}\n\n` +
        `Fix correctness/fidelity/process issues; do NOT weaken tests or invent shortcuts; mark any deliberately ` +
        `omitted part // STUB with a reason. Do NOT commit. Report what you changed and re-run the deliverable's ` +
        `check (cargo build/test where applicable) with PATH=\`$HOME/.cargo/bin:$PATH\`.`,
      { label: `fix:${SLUG}:r${r}`, phase: 'Fix' },
    )
  }
}
log(`${all.length} findings after ${MAX} rounds — not converged (controller triage)`)
return { merge_ready: false, findings: all, worktree: WT }
