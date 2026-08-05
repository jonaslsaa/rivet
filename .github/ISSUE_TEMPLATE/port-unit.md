---
name: Port unit (sub-issue)
about: One manifest unit of translation work, created under an epic
title: "port: <unit-id>"
labels: []
---

**Epic:** #<!-- epic number -->
**Manifest unit(s):** `<unit-id>` (`<java_package>`, N files, M LOC, wave W)
**Target:** `crates/<crate>/src/<module>`

**Scope notes** <!-- anything unusual: cycles with other units, stubs expected, Java tests present -->

**Done means:** translated per `PORTING.md`, reviewed (both lenses), `cargo check -p <crate>` clean
or errors handed to burndown, Java tests ported, `MANIFEST.tsv` updated in the PR.
