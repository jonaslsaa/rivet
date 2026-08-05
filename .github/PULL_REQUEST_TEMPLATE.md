## What

<!-- One paragraph. Link the epic and the manifest unit(s): "Ports mc.nbt (epic #5)". -->

## Fidelity checklist (PORTING.md)

- [ ] Translated from the Java source in `working/Paper` (not from memory, not from Pumpkin)
- [ ] Wrapping arithmetic on all Java int/long math; `>>>` mapped correctly
- [ ] No dropped null-checks; no invented or "improved" logic
- [ ] HashMap iteration order not observable (or IndexMap/sort used)
- [ ] Existing Java tests ported alongside; no test/fixture weakened
- [ ] `MANIFEST.tsv` status updated for the unit(s)
- [ ] Any `// STUB(...)` items created are listed below

## Stubs / blocked / open questions

<!-- List cross-unit stubs created, todo!()-with-reason sites, and reviewer disagreements. -->
