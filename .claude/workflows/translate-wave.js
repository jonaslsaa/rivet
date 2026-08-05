export const meta = {
  name: 'translate-wave',
  description: 'Translate a wave of Rivet manifest units from Java to Rust, looping adversarial review until clean',
  whenToUse: 'One port wave. Pass args {waveId, units: [{id, java_package, source_root, files, crate, notes?}]} built from MANIFEST.tsv rows by the controller.',
  phases: [{ title: 'Translate' }, { title: 'Review' }, { title: 'Fix' }],
}

const MAX_REVIEW_ROUNDS = 3

const IMPL_REPORT = {
  type: 'object',
  required: ['unit', 'status', 'files_written', 'summary'],
  properties: {
    unit: { type: 'string' },
    status: { enum: ['translated', 'blocked'] },
    blocked_reason: { type: 'string' },
    files_written: { type: 'array', items: { type: 'string' } },
    tests_written: { type: 'array', items: { type: 'string' } },
    stubs_created: { type: 'array', items: { type: 'string' }, description: 'out-of-unit stub items created to satisfy cross-unit references' },
    open_questions: { type: 'array', items: { type: 'string' } },
    summary: { type: 'string' },
  },
}

const FINDINGS = {
  type: 'object',
  required: ['findings'],
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
  },
}

const COMMON_RULES = `
Repository root: the current working directory. Source of truth for this unit is the Java
code under working/Paper. HARD RULES (from GOAL.md — read it, plus PORTING.md, before writing code):
- NEVER run git commit, git stash, git reset, git checkout, or push. Only read/write files.
- Never modify: MANIFEST.tsv, any *.md design doc, fixtures, or files outside your unit's
  crate (except pure stub declarations for cross-unit types, marked with // STUB(unit-id)).
- Wrapping arithmetic for all Java int/long math; >>> is a logical shift; exact RNG and
  Mth tables; UTF-16-aware string indices where observable. See PORTING.md drift checklist.
- No todo!() without status=blocked + blocked_reason. Never invent APIs or "improve" logic.
- Never copy from working/Pumpkin.`

const LENSES = {
  'semantic-drift': `LENS — SEMANTIC DRIFT. Hunt ONLY translation bugs, using PORTING.md's drift checklist:
wrapping arithmetic omissions, >>> vs >>, HashMap iteration-order observability, UTF-16 index
drift, f32/f64 widening, Mth-table vs libm, dropped null checks, eager unwrap_or, identity-vs-
value equality, dropped synchronized without a note, debug_assert side effects, cast semantics,
inclusive/exclusive off-by-ones. Compare Rust line-by-line against the Java original.`,
  'ownership-api': `LENS — OWNERSHIP & API FIDELITY. Verify the code follows OWNERSHIP.md (arenas + IDs, no
Arc/RwLock on game state, sync-tick assumptions), PORTING.md naming/module conventions, that no
Java logic or method was silently skipped or invented (diff the public surface against the Java
package), that stubs are minimal and marked, and that tests were not weakened or fabricated.`,
  combined: `FULL LENS. This unit was already reviewed and fixed at least once; you are a FRESH
verifier with no knowledge of prior findings — review the current code state completely. Apply
BOTH the PORTING.md drift checklist (wrapping arithmetic, >>> handling, map-order observability,
UTF-16 indices, float widening, Mth tables, null checks, equality semantics, cast semantics)
AND the architecture rules (OWNERSHIP.md arenas/IDs, no invented or skipped logic vs the Java
original, minimal marked stubs, tests not weakened).`,
}

function implementPrompt(u) {
  return `You are the IMPLEMENTER for Rivet port unit "${u.id}" (wave job).
${COMMON_RULES}

Unit: Java package ${u.java_package} (${u.files} files) in working/Paper (source root: ${u.source_root}).
Target: crate crates/${u.crate}, module path mirroring the Java package per PORTING.md naming.
${u.notes ? `Unit notes: ${u.notes}` : ''}

Steps:
1. Read GOAL.md, PORTING.md, and the relevant OWNERSHIP.md section for this crate.
2. Read every Java file of the package; also skim its direct dependencies' public signatures.
3. Translate faithfully per PORTING.md into crates/${u.crate}/src/... — preserve structure,
   names, algorithms, constants. Cross-unit types not yet ported: declare minimal stubs in the
   owning crate marked // STUB(${u.id}) and list them in stubs_created.
4. If the Java package has JUnit tests in working/Paper, port them alongside. If the unit has a
   serialization/math surface, add round-trip or fixture-based tests (do not invent behavioral
   tests for gameplay logic).
5. Run cargo check -p ${u.crate} once at the end; fixing all errors is NOT required (burndown
   is a later phase), but report remaining error count in summary.

Your final output must be the structured report. files_written must be exhaustive.`
}

function reviewPrompt(u, report, lens) {
  return `You are a REVIEWER for Rivet port unit "${u.id}". You must NOT see or trust the
implementer's reasoning — judge only the code. ${LENSES[lens]}

Inputs: the Java originals (package ${u.java_package} under working/Paper) and the written Rust
files: ${JSON.stringify(report.files_written)}. Read both fully. Report every defect as a finding
(severity: critical = wrong behavior/parity break, major = convention/architecture violation,
minor = style). An empty findings list is a valid result — do not manufacture findings.`
}

function fixPrompt(u, report, findings) {
  return `You are the FIXER for Rivet port unit "${u.id}".
${COMMON_RULES}

Apply these reviewer findings to the files ${JSON.stringify(report.files_written)}:
${JSON.stringify(findings, null, 1)}

Rules: fix all critical and major findings; apply minor ones when unambiguous. If a finding is
WRONG (reviewer misread the Java), do not apply it — record why in open_questions. Verify each
fix against the Java original in working/Paper. Re-run cargo check -p ${u.crate} once; report
remaining error count in summary. Return the updated structured report.`
}

async function reviewRound(u, report, round) {
  // Round 1 gets both lenses (highest yield); later rounds one fresh full-lens verifier.
  const lenses = round === 1 ? ['semantic-drift', 'ownership-api'] : ['combined']
  const results = await parallel(lenses.map(lens => () =>
    agent(reviewPrompt(u, report, lens), {
      label: `rev:${u.id}:r${round}${lenses.length > 1 ? `:${lens}` : ''}`,
      phase: 'Review',
      schema: FINDINGS,
      effort: 'high',
    })))
  return results.filter(Boolean).flatMap(r => r.findings)
}

async function converge(u, impl) {
  let report = impl
  let totalFindings = 0
  for (let round = 1; round <= MAX_REVIEW_ROUNDS; round++) {
    const findings = await reviewRound(u, report, round)
    if (findings.length === 0) {
      return { ...report, review_rounds: round, review_findings: totalFindings, converged: true }
    }
    totalFindings += findings.length
    const lastRound = round === MAX_REVIEW_ROUNDS
    const onlyMinor = findings.every(f => f.severity === 'minor')
    if (lastRound && !onlyMinor) {
      return {
        ...report,
        status: 'blocked',
        blocked_reason: `did not converge: ${findings.length} findings after ${MAX_REVIEW_ROUNDS} review rounds`,
        review_rounds: round,
        review_findings: totalFindings,
        converged: false,
      }
    }
    const fixed = await agent(fixPrompt(u, report, findings), {
      label: `fix:${u.id}:r${round}`, phase: 'Fix', schema: IMPL_REPORT,
    })
    if (!fixed || fixed.status === 'blocked') return fixed
    report = fixed
    if (lastRound) {
      // Minor-only tail: fixed but unverified — converged with a caveat, not blocked.
      return { ...report, review_rounds: round, review_findings: totalFindings, converged: 'with-unverified-minor-fixes' }
    }
  }
}

if (!args || !Array.isArray(args.units) || args.units.length === 0) {
  throw new Error('translate-wave requires args {waveId, units: [...]}')
}
log(`wave ${args.waveId}: ${args.units.length} units`)

const results = await pipeline(
  args.units,
  u => agent(implementPrompt(u), { label: `impl:${u.id}`, phase: 'Translate', schema: IMPL_REPORT }),
  (impl, u) => {
    if (!impl || impl.status === 'blocked') return impl
    return converge(u, impl)
  }
)

const done = results.filter(Boolean)
const blocked = done.filter(r => r.status === 'blocked')
log(`wave ${args.waveId} complete: ${done.length}/${args.units.length} reported, ${blocked.length} blocked`)
return { waveId: args.waveId, results: done }
