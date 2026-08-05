export const meta = {
  name: 'review-pr',
  description: 'Pre-merge heavyweight review of a whole PR diff (cross-unit consistency), then the controller runs gate.sh',
  whenToUse: 'Run once per PR after check-burndown converges, before merging. Pass args {pr} (number) or {range} (git range like main...HEAD).',
  phases: [{ title: 'Nuke' }],
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
  },
}

if (!args || (!args.pr && !args.range)) throw new Error('review-pr requires args {pr} or {range}')
const target = args.pr ? `PR #${args.pr} (use \`gh pr diff ${args.pr}\` and \`gh pr view ${args.pr}\`)` : `git range ${args.range} (use \`git diff ${args.range}\`)`

const verdict = await agent(
  `You are the pre-merge reviewer for the Rivet port. Review the ENTIRE diff of ${target} as one
body of work. Per-unit reviews already ran and converged — do NOT re-litigate line-level style.
Your job is what per-unit reviews structurally cannot see:
- Cross-unit consistency: duplicated or conflicting // STUB declarations, divergent naming for
  the same Java concept, incompatible module boundaries.
- OWNERSHIP.md conformance across files: any Arc/RwLock/interior mutability creeping into game
  state, back-references stored instead of IDs, per-unit deviations that only clash in aggregate.
- API surface coherence vs the Java originals in working/Paper (spot-check the riskiest files).
- Process violations: weakened tests or fixtures, MANIFEST.tsv edits beyond status columns,
  design-doc edits smuggled into the PR, todo!() without a blocked note.
Read GOAL.md, PORTING.md, OWNERSHIP.md first. Set merge_ready=false if any critical or major
finding exists. Do not manufacture findings — an empty list with merge_ready=true is a valid
result for a good PR.`,
  { label: `nuke:${args.pr || args.range}`, phase: 'Nuke', schema: VERDICT, effort: 'max' },
)

if (!verdict) throw new Error('review agent failed')
log(verdict.merge_ready ? 'merge-ready (run scripts/gate.sh next)' : `${verdict.findings.length} findings — not merge-ready`)
return verdict
