export const meta = {
  name: 'check-burndown',
  description: 'Burn down cargo check errors crate-by-crate after a translate wave',
  whenToUse: 'Run after translate-wave until the workspace compiles. Optional args {maxRounds}.',
  phases: [{ title: 'Survey' }, { title: 'Fix' }],
}

const ERRORS = {
  type: 'object',
  required: ['clean', 'groups'],
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
correcting the translation against the Java original in working/Paper (PORTING.md governs).
Never weaken types to () or Box<dyn Any> as an escape hatch.`

const maxRounds = (args && args.maxRounds) || 8
const FALLBACK_MODEL = args && args.fallbackModel
let lastTotal = Infinity

// A stalled provider call or mid-stream disconnect must cost one retry, not the
// whole run or a silently skipped crate: agent() throws on harness-level stall
// and returns null on terminal API errors. The attempt preamble also makes each
// retry a distinct journal key, so a resumed run never replays a half-dead
// attempt as completed.
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

for (let round = 1; round <= maxRounds; round++) {
  const survey = await resilientAgent(
    `Run \`cargo check --workspace --message-format=short 2>&1\` in the repo root. Group the
errors by crate (and dominant module within it). Return the structured summary — clean=true only
if there are zero errors (warnings do not count).`,
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
${RULES}
This workflow gets paused and resumed: the crate may contain partial fixes from an interrupted
prior attempt — re-run cargo check first and work from the current state.
Currently ~${g.error_count} errors. Sample:
${g.sample}
Work until \`cargo check -p ${g.crate}\` is clean or you cannot proceed without violating the
rules; list unresolvable items in gave_up_on. Return the structured report.`,
      { label: `burn:${g.crate}:r${round}`, phase: 'Fix', schema: FIX_REPORT, effort: 'low' },
    )))
}

return { clean: false, rounds: maxRounds, note: 'max rounds reached' }
