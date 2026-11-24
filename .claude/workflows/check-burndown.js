export const meta = {
  name: 'check-burndown',
  description: 'Burn down cargo check errors crate-by-crate after a translate wave',
  whenToUse: 'Run after translate-wave until the workspace compiles. Pass {worktree, maxRounds?, fixModel?, fixEffort?}. worktree is the absolute path of the lane worktree (pinned in every prompt because workflow agents inherit the session cwd); fixers run on fixModel/fixEffort (default sonnet/high).',
  phases: [{ title: 'Survey' }, { title: 'Fix' }],
}

// args sometimes arrives JSON-encoded as a string instead of an object.
const A = typeof args === 'string' ? JSON.parse(args) : args
if (!A || !A.worktree) throw new Error('check-burndown requires {worktree}')
// Workflow agents inherit the SESSION cwd, so every prompt pins the worktree.
const WT = String(A.worktree).replace(/\/+$/, '')
if (!WT.startsWith('/')) throw new Error(`check-burndown: worktree must be an absolute path (got ${WT})`)
// Lane worktrees live at <main checkout>/.claude/worktrees/<name>; the Paper
// source tree is the main checkout's working/Paper (a worktree may lack the symlink).
const WT_MARK = '/.claude/worktrees/'
const PAPER = `${WT.includes(WT_MARK) ? WT.slice(0, WT.indexOf(WT_MARK)) : WT}/working/Paper`
const FIX_OPTS = { model: A.fixModel || 'sonnet', effort: A.fixEffort || 'high' }
const CARGO = 'PATH=$HOME/.cargo/bin:$PATH CARGO_BUILD_JOBS=4'
const WHERE = `Work ONLY in the git worktree \`${WT}\`: your shell does not start there, so prefix every command with
\`cd ${WT} &&\` and use absolute paths under ${WT} for every file you read or write. Run cargo as
\`${CARGO} cargo <cmd> --locked\`. Never build under /tmp.`

const ERRORS = {
  type: 'object',
  required: ['clean', 'total_errors', 'groups'],
  properties: {
    clean: { type: 'boolean' },
    total_errors: { type: 'integer' },
    groups: {
      type: 'array',
      items: {
        type: 'object',
        required: ['crate', 'error_count', 'sample'],
        properties: {
          crate: { type: 'string' },
          module: { type: 'string' },
          error_count: { type: 'integer' },
          sample: { type: 'string', description: 'representative error messages, truncated' },
        },
      },
    },
  },
}

const FIX_REPORT = {
  type: 'object',
  required: ['crate', 'errors_before', 'errors_after', 'summary'],
  properties: {
    crate: { type: 'string' },
    errors_before: { type: 'integer' },
    errors_after: { type: 'integer' },
    gave_up_on: { type: 'array', items: { type: 'string' } },
    summary: { type: 'string' },
  },
}

const RULES = `HARD RULES: never git commit/stash/reset/push; never delete functionality, tests, or
whole items to silence an error; never add todo!()/unimplemented!() — if an error can only be fixed
by changing another crate's stub, extend that stub minimally (marked // STUB). Fix errors by
correcting the translation against the Java original in ${PAPER} (read-only; PORTING.md governs).
Never weaken types to () or Box<dyn Any> as an escape hatch. Never define a second copy or local
stand-in of a type that already exists elsewhere in the workspace — import the canonical one. Never
change an expected value, fixture, golden, or comparator to resolve a compile or test failure.`

const maxRounds = A.maxRounds || 8
let lastTotal = Infinity

// A stalled provider call must cost one retry, not the whole run or a silently
// skipped crate: agent() throws on harness-level stall exhaustion and returns
// null on terminal API errors. One extra attempt only — the harness already
// retries internally. Never retry a user abort. The retry preamble makes the
// attempt a distinct journal key, so a resumed run never replays a half-dead
// attempt as completed.
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

for (let round = 1; round <= maxRounds; round++) {
  const survey = await resilientAgent(
    `${WHERE}
Run \`cd ${WT} && ${CARGO} cargo check --workspace --locked --message-format=short 2>&1\`. Do not edit
anything. Group the errors by crate (and dominant module within it). Return the structured summary —
clean=true only if there are zero errors (warnings do not count).`,
    { label: `survey:r${round}`, phase: 'Survey', schema: ERRORS, effort: 'low' },
  )
  if (!survey) {
    log('survey failed after retries — stopping for controller triage')
    return { clean: false, survey_failed: true, rounds: round }
  }
  if (survey.clean) { log(`round ${round}: workspace clean`); return { clean: true, rounds: round } }
  log(`round ${round}: ${survey.total_errors} errors in ${survey.groups.length} crates`)
  if (survey.total_errors >= lastTotal && round > 2) {
    log('no progress for a round — stopping for controller triage')
    return { clean: false, stalled: true, groups: survey.groups }
  }
  lastTotal = survey.total_errors

  await parallel(survey.groups.map(g => () =>
    resilientAgent(
      `Fix the cargo check errors in crate ${g.crate}${g.module ? ` (focus: ${g.module})` : ''}.
${WHERE}
${RULES}
This workflow gets paused and resumed: the crate may contain partial fixes from an interrupted
prior attempt — re-run cargo check first and work from the current state.
Currently ~${g.error_count} errors. Sample:
${g.sample}
Work until \`cd ${WT} && ${CARGO} cargo check -p ${g.crate} --locked\` is clean or you cannot proceed without violating the
rules; list unresolvable items in gave_up_on. Return the structured report.`,
      { label: `burn:${g.crate}:r${round}`, phase: 'Fix', schema: FIX_REPORT, ...FIX_OPTS },
    )))
}

return { clean: false, rounds: maxRounds, note: 'max rounds reached' }
