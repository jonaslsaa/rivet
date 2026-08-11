export const meta = {
  name: 'release-prep',
  description: 'Refresh an already-implemented worktree onto current main, run focused checks, and loop fresh exact-head review and fixes until it is ready for the serialized release gate',
  whenToUse: 'Use after implementation is complete but before the coordinator-owned strict gate. Pass {worktree, task, maxRounds?, checks?}. The workflow may fetch, rebase, resolve conflicts, fix findings, and commit locally; it never runs the full gate, pushes, opens a PR, or merges.',
  phases: [{ title: 'Refresh' }, { title: 'Check' }, { title: 'Review' }, { title: 'Fix' }],
}

const A = typeof args === 'string' ? JSON.parse(args) : args
if (!A || !A.worktree || !A.task) throw new Error('release-prep requires {worktree, task}')
const WT = A.worktree
const TASK = A.task
const MAX = A.maxRounds ?? 6
const CHECKS = A.checks || ''
const SLUG = WT.split('/').pop()

const REFRESH = {
  type: 'object',
  required: ['ready', 'head', 'base', 'ahead', 'behind', 'clean', 'files', 'summary'],
  properties: {
    ready: { type: 'boolean' },
    head: { type: 'string' },
    base: { type: 'string' },
    ahead: { type: 'integer' },
    behind: { type: 'integer' },
    clean: { type: 'boolean' },
    files: { type: 'array', items: { type: 'string' }, description: 'repo-relative deliverable files in the merge-base diff' },
    summary: { type: 'string' },
    blocker: { type: 'string' },
  },
}

const CHECK = {
  type: 'object',
  required: ['pass', 'head', 'base', 'commands', 'output'],
  properties: {
    pass: { type: 'boolean' },
    head: { type: 'string' },
    base: { type: 'string' },
    commands: { type: 'array', minItems: 1, items: { type: 'string' } },
    output: { type: 'string', minLength: 1 },
  },
}

const VERDICT = {
  type: 'object',
  required: ['merge_ready', 'head', 'base', 'ahead', 'behind', 'clean', 'findings', 'summary', 'files_inspected'],
  properties: {
    merge_ready: { type: 'boolean' },
    head: { type: 'string' },
    base: { type: 'string' },
    ahead: { type: 'integer' },
    behind: { type: 'integer' },
    clean: { type: 'boolean' },
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
    files_inspected: { type: 'array', minItems: 1, items: { type: 'string' }, description: 'repo-relative paths' },
  },
}

const FIX_REPORT = {
  type: 'object',
  required: ['status', 'summary', 'head', 'clean', 'changed', 'disputed', 'checks'],
  properties: {
    status: { enum: ['fixed', 'blocked'] },
    summary: { type: 'string' },
    blocker: { type: 'string' },
    head: { type: 'string' },
    clean: { type: 'boolean' },
    changed: { type: 'array', items: { type: 'string' } },
    disputed: {
      type: 'array',
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
    checks: { type: 'string' },
  },
}

async function resilientAgent(prompt, opts, attempts = 2) {
  for (let attempt = 1; attempt <= attempts; attempt++) {
    const attemptPrompt = attempt === 1
      ? prompt
      : `(Retry ${attempt}/${attempts}: the previous attempt stalled, failed, or returned no result. Re-read the CURRENT on-disk state and redo the task; do not assume the first attempt made no changes.)\n\n${prompt}`
    try {
      const result = await agent(attemptPrompt, opts)
      if (result !== null) return result
      log(`${opts.label}: attempt ${attempt}/${attempts} returned no result`)
    } catch (e) {
      const message = `${(e && e.message) || e}`
      if (/abort/i.test(message)) throw e
      log(`${opts.label}: attempt ${attempt}/${attempts} failed: ${message}`)
    }
  }
  return null
}

const HARD_RULES =
  `Never git stash/reset/force-push, never push, never open or merge a PR, and never run ` +
  `\`scripts/gate.sh\` or \`scripts/gate.sh --require-oracle\`; the coordinator owns the serialized strict gate and release. ` +
  `Never weaken tests or fixtures. Never abort a rebase or discard conflicting/unexplained files; resolve in place or stop blocked. ` +
  `Never modify or commit anything from \`${WT}/working\`.`

const RESUME_NOTE =
  `This workflow may resume after interruption. Re-read branch, status, log, and rebase state from disk before acting. ` +
  `If a rebase is already in progress, resolve it faithfully and continue it; never abort it or assume a clean starting state.`

const REVIEW_SCOPE =
  `In \`${WT}\`, fetch origin, record full 40-character hashes from \`git rev-parse HEAD\` and \`git rev-parse origin/main\`, then inspect ` +
  `\`git log --oneline origin/main..HEAD\`, \`git diff $(git merge-base origin/main HEAD) HEAD\`, and \`git status --short\`. ` +
  `Read every changed and untracked deliverable file. A dirty tree, untracked deliverable, detached HEAD, zero commits ahead, ` +
  `or any commit behind current origin/main is a release-blocking process finding. You are READ-ONLY: do not edit files, ` +
  `commit, rebase, or create scratch files in the worktree. List every file actually read as a repo-relative path in files_inspected.`

function processFinding(description) {
  return [{ file: '(git)', description, severity: 'critical' }]
}

function fixPrompt(findings, disputes) {
  return (
    `Prepare the already-implemented branch in \`${WT}\` for release. Task/deliverable:\n${TASK}\n\n` +
    `Apply or resolve these findings:\n${JSON.stringify(findings, null, 2)}\n\n` +
    `Prior disputes, if any:\n${JSON.stringify(disputes || [], null, 2)}\n\n` +
    `${HARD_RULES} Work only from the current state. Inspect all uncommitted files before acting: commit them only if they ` +
    `belong to this task and are correct; never delete or overwrite unexplained work. Correct confirmed findings against ` +
    `project docs and pinned Paper where relevant. If a finding is factually wrong, do not apply it; record a precise dispute. ` +
    `Reconsider comments before committing. Run focused checks, commit all intended deliverable changes conventionally, and ` +
    `finish with a clean tree. Report the resulting full HEAD hash, cleanliness, changed files, disputes, and check evidence. ` +
    `If safe completion is impossible, return status=blocked and preserve the state.\n` +
    `${CHECKS ? `Required focused checks:\n${CHECKS}` : ''}`
  )
}

let all = []
let prior = []
let disputes = []

for (let round = 1; round <= MAX; round++) {
  phase('Refresh')
  const refresh = await resilientAgent(
    `Refresh the already-implemented branch in \`${WT}\` onto current \`origin/main\` before release review. ` +
      `${HARD_RULES} ${RESUME_NOTE} Run \`git fetch origin\`. If the tree is clean, rebase onto ` +
      `\`origin/main\` when behind; resolve any rebase conflicts faithfully using the task and current main, then continue the ` +
      `rebase without weakening or dropping the deliverable. If the tree is dirty, do not rebase or discard anything: report ` +
      `ready=false so the fix phase can inspect and commit legitimate task work. Finish by reporting full 40-character hashes, ` +
      `ahead/behind counts, cleanliness, and every merge-base-diff deliverable file as a repo-relative path. ready=true requires ` +
      `a named feature/fix/chore/refactor branch, clean tree, ` +
      `ahead>0, and behind=0. Task:\n${TASK}`,
    { label: `refresh:${SLUG}:r${round}`, phase: 'Refresh', schema: REFRESH },
  )
  if (!refresh) {
    log(`refresh r${round} failed after retries`)
    return { merge_ready: false, strict_gate_ready: false, refresh_failed: true, findings: all, rounds: round, worktree: WT }
  }
  if (!refresh.ready) {
    const findings = processFinding(refresh.blocker || refresh.summary || 'branch refresh did not reach a release-reviewable state')
    all = all.concat(findings.map((finding) => ({ ...finding, round })))
    if (round === MAX) break
    phase('Fix')
    const fixed = await resilientAgent(
      fixPrompt(findings, disputes),
      { label: `fix-refresh:${SLUG}:r${round}`, phase: 'Fix', schema: FIX_REPORT },
    )
    if (!fixed || fixed.status === 'blocked' || !fixed.clean) {
      log(`refresh fix r${round} did not complete cleanly`)
      return { merge_ready: false, strict_gate_ready: false, fix_failed: !fixed || !fixed.clean, blocked: !!fixed && fixed.status === 'blocked', findings: all, rounds: round, worktree: WT }
    }
    prior = findings
    disputes = fixed.disputed
    continue
  }

  phase('Check')
  const check = await resilientAgent(
    `Run focused mechanical checks for the exact current head in \`${WT}\`. ${HARD_RULES} Run \`git fetch origin\` first, ` +
      `then determine touched files and crates from the merge-base diff. Stop at the first failure. For Rust run cargo fmt ` +
      `--check, clippy with -Dwarnings, ` +
      `check, and tests scoped to every touched crate/all relevant targets. For changed shell run bash -n and shellcheck if ` +
      `installed, plus safe focused tests. For changed Python run py_compile and focused tests. Validate generated/manifest ` +
      `artifacts when touched. Do not edit anything. Report the full 40-character HEAD and origin/main hashes observed after ` +
      `the fetch and before the checks, ` +
      `every command, and concise failure output or "all green".\n${CHECKS ? `Required checks:\n${CHECKS}` : ''}`,
    { label: `check:${SLUG}:r${round}`, phase: 'Check', schema: CHECK, effort: 'low' },
  )
  if (!check) {
    log(`check r${round} failed after retries`)
    return { merge_ready: false, strict_gate_ready: false, check_failed: true, findings: all, rounds: round, worktree: WT }
  }
  if (check.head !== refresh.head || check.base !== refresh.base) {
    const findings = processFinding(`head/base changed during preparation: refresh ${refresh.head}/${refresh.base}, check ${check.head}/${check.base}`)
    all = all.concat(findings.map((finding) => ({ ...finding, round })))
    prior = findings
    disputes = []
  } else if (!check.pass || check.commands.length === 0) {
    const findings = [{ file: '(build)', description: `focused mechanical checks failed or ran no commands:\n${check.output}`, severity: 'critical' }]
    all = all.concat(findings.map((finding) => ({ ...finding, round })))
    prior = findings
    disputes = []
  } else {
    phase('Review')
    const verdict = await resilientAgent(
      `${round === 1
        ? 'You are a FRESH adversarial release reviewer. Do a complete independent review.'
        : `You are a FRESH full-lens release reviewer. Review the whole CURRENT diff independently FIRST, then verify prior findings and disputes.\nPrior findings:\n${JSON.stringify(prior, null, 2)}\nDisputes:\n${JSON.stringify(disputes, null, 2)}`}\n\n` +
        `Task/deliverable:\n${TASK}\n\n${REVIEW_SCOPE} Check correctness, fidelity to pinned Paper and project docs where relevant, ` +
        `test strength, generated artifacts, and process integrity. Report only concrete actionable defects, not preferences. ` +
        `The refresh phase reports these repo-relative deliverable files, all of which must appear in files_inspected: ` +
        `${JSON.stringify(refresh.files)}. merge_ready=true requires: exact reviewed HEAD=${refresh.head}, base=${refresh.base}, ` +
        `clean=true, ahead>0, behind=0, every changed file inspected, and an EMPTY findings array. Any finding of any severity ` +
        `means merge_ready=false. The full strict gate was intentionally ` +
        `not run and must not be requested as a finding; it is the coordinator's next step after this workflow.`,
      { label: `review:${SLUG}:r${round}`, phase: 'Review', schema: VERDICT, effort: 'high' },
    )
    if (!verdict) {
      log(`review r${round} failed after retries`)
      return { merge_ready: false, strict_gate_ready: false, review_failed: true, findings: all, rounds: round, worktree: WT }
    }
    all = all.concat(verdict.findings.map((finding) => ({ ...finding, round })))
    const exactState = verdict.head === refresh.head && verdict.base === refresh.base
    const inspectionComplete = refresh.files.length > 0 && refresh.files.every((file) => verdict.files_inspected.includes(file))
    const consistentReady = verdict.merge_ready && verdict.findings.length === 0 && verdict.clean && verdict.ahead > 0 && verdict.behind === 0 && exactState && inspectionComplete
    if (verdict.merge_ready && !consistentReady) {
      log(`review r${round} returned an inconsistent merge-ready verdict; treating it as not ready`)
    }
    if (consistentReady) {
      log(`release preparation converged after ${round} round(s) at ${verdict.head}`)
      return {
        merge_ready: true,
        strict_gate_ready: true,
        strict_gate_run: false,
        head: verdict.head,
        base: verdict.base,
        ahead: verdict.ahead,
        behind: verdict.behind,
        clean: verdict.clean,
        checks: check.commands,
        review: verdict.summary,
        findings: all,
        rounds: round,
        worktree: WT,
      }
    }
    if (verdict.findings.length > 0) {
      prior = verdict.findings
      disputes = []
    } else {
      prior = processFinding(
        `review state/verdict was not release-ready (exactState=${exactState}, inspectionComplete=${inspectionComplete}): ${verdict.summary}`,
      )
      all = all.concat(prior.map((finding) => ({ ...finding, round })))
      disputes = []
    }
  }

  if (round === MAX) break
  phase('Fix')
  const fixed = await resilientAgent(
    fixPrompt(prior, disputes),
    { label: `fix:${SLUG}:r${round}`, phase: 'Fix', schema: FIX_REPORT },
  )
  if (!fixed || fixed.status === 'blocked' || !fixed.clean) {
    log(`fix r${round} did not complete cleanly`)
    return { merge_ready: false, strict_gate_ready: false, fix_failed: !fixed || !fixed.clean, blocked: !!fixed && fixed.status === 'blocked', findings: all, rounds: round, worktree: WT }
  }
  disputes = fixed.disputed
}

log(`release preparation did not converge after ${MAX} rounds`)
return { merge_ready: false, strict_gate_ready: false, findings: all, rounds: MAX, worktree: WT }
