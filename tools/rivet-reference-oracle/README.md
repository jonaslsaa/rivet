# rivet-reference-oracle

A small JSON Lines CLI that exposes deterministic behavior from the pinned
Paper 26.2 Java implementation. Rivet tests can send the same input to this
process and the Rust port, then compare structured responses instead of
hand-writing expected values.

The tool currently supports:

- `ping` — verify the process and protocol version.
- `snbt.parse` — parse any SNBT tag and return compact and pretty canonical
  forms plus its Java tag type.
- `nbt.encode` — parse a compound SNBT root and return its uncompressed binary
  NBT representation as base64.
- `nbt.decode` — decode uncompressed compound NBT from base64 and return its
  canonical SNBT representation.

It compiles against `working/Paper/paper-server/build/libs/paper-server-*.jar`
and uses libraries materialized by the M0 Paper run under
`tools/rivet-oracle/work/run/libraries`. Both locations can be overridden with
`RIVET_PAPER_JAR` and `RIVET_PAPER_LIBRARIES`; when overriding them, point
`RIVET_PAPER_RUNTIME_JAR` at the Paper jar materialized beside those runtime
libraries. The launcher requires the compile and runtime jars to have the same
SHA-256 and verifies their manifest identifies Paper 26.2.
It also requires the jar's Git commit to match the canonical `paper` revision
in `tools/rivet-oracle/fixtures/manifest.json`; changing the reference server
therefore requires deliberate fixture regeneration and re-pinning.
Paper 26.2 requires Java 25. The launcher finds a Java 25 SDKMAN installation
or uses `JAVA_HOME`; set `RIVET_JAVA_HOME` when neither points to the required
JDK.

Start the persistent oracle:

```sh
tools/rivet-reference-oracle/run.sh
```

Every non-empty stdin line must be one JSON object. Every stdout line is one
JSON response carrying the request's optional `id`:

```json
{"id":"example","op":"snbt.parse","input":"{answer:42,ok:true}"}
{"id":"example","ok":true,"result":{"tag_id":10,"tag_type":"COMPOUND","snbt":"{answer:42,ok:1b}","pretty_snbt":"{\n    answer: 42,\n    ok: 1b\n}"},"protocol":1}
```

Run the built-in smoke test without starting a persistent session:

```sh
tools/rivet-reference-oracle/run.sh --self-test
```

Diagnostics and compilation output go to stderr. Stdout is reserved for the
JSON Lines protocol. Build artifacts stay in the tool's ignored `target/`
directory; no Paper or Mojang-derived source is copied into Rivet.
