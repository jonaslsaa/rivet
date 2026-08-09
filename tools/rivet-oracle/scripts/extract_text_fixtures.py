#!/usr/bin/env python3
"""Capture the Paper-grounded text fixture golden for issue #98.

Reads `corpus.json` (the committed component-JSON corpus) and drives the Paper
reference oracle (`tools/rivet-reference-oracle/run.sh`, which compiles against
the pinned Paper 26.2 jar) over the `component.json` operation, writing
`golden.json`: for every entry, whether Paper's `ComponentSerialization.CODEC`
accepts it and the exact canonical JSON the codec re-emits under
non-compressed `JsonOps` (a chat/title/player-info/scoreboard wire form).

The golden `canonical` is stored as a JSON *string* copied verbatim from the
oracle, so no serialization layer re-normalizes it: the bytes Paper produces
are the bytes the committed fixture records, and the Rust side compares its own
re-encode against that exact string (issue #98: byte/JSON identity without
lossy normalization). `accept:false` entries record only the reject verdict
(the strict-malformed fixtures). The `manifest.json` is written by the Rust
runner (`regenerate --text`), not here, so regeneration stays byte-identical
and unit-testable without a JVM.

The oracle runtime is the M0-materialized Paper libraries/jar; the same env
vars `run.sh` honours apply (`RIVET_PAPER_JAR`, `RIVET_PAPER_LIBRARIES`,
`RIVET_PAPER_RUNTIME_JAR`).

Usage:
  python3 scripts/extract_text_fixtures.py <corpus.json> <out-golden.json>
"""
import json
import subprocess
import sys
from pathlib import Path


def main() -> int:
    corpus_path = Path(sys.argv[1])
    golden_path = Path(sys.argv[2])
    script_dir = Path(__file__).resolve().parent
    run_sh = script_dir.parent.parent / "rivet-reference-oracle" / "run.sh"
    if not run_sh.is_file():
        print(f"reference oracle launcher missing: {run_sh}", file=sys.stderr)
        return 1

    corpus = json.loads(corpus_path.read_text(encoding="utf-8"))
    entries = corpus["entries"]

    # One JVM boot: a leading `ping` learns the pin of the Paper jar the oracle
    # actually compiled against (recorded as the golden's `paper` provenance so
    # the manifest never goes stale), then every entry as a JSON-Lines request.
    payload = "\n".join(
        [json.dumps({"id": "ping", "op": "ping"})]
        + [
            json.dumps({"id": str(i), "op": "component.json", "input": e["input"]})
            for i, e in enumerate(entries)
        ]
    ) + "\n"
    proc = subprocess.run(
        [str(run_sh)], input=payload, text=True, capture_output=True, encoding="utf-8"
    )
    if proc.returncode != 0:
        sys.stderr.write(proc.stderr)
        return 1

    # Parse the oracle's JSON-Lines response. The reference oracle writes ONLY
    # its JSON responses to stdout (it rides the raw stream, not log4j), but a
    # launcher or JVM regression could leak a log line — that must fail loudly
    # with a clear message, never be silently skipped or misread as a response.
    responses = {}
    for line in proc.stdout.splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            resp = json.loads(line)
        except json.JSONDecodeError as error:
            print(
                f"oracle stdout is not pure JSON-Lines; unparseable line: {line!r} "
                f"({error})",
                file=sys.stderr,
            )
            return 1
        rid = resp.get("id")
        if rid is None:
            print(f"oracle response missing `id`: {resp}", file=sys.stderr)
            return 1
        if str(rid) in responses:
            print(
                f"oracle emitted a duplicate response id {rid!r}: {resp}",
                file=sys.stderr,
            )
            return 1
        responses[str(rid)] = resp

    ping = responses.pop("ping", None)
    if ping is None or not ping.get("ok"):
        print("oracle ping failed; cannot record the captured Paper pin", file=sys.stderr)
        return 1
    ping_result = ping.get("result", {})
    impl = ping_result.get("paper_implementation", "unknown")
    commit = ping_result.get("paper_commit", "unknown")
    paper = f"{impl}@{commit}"

    golden = {"format": 1, "kind": "text", "paper": paper, "entries": []}
    for i, entry in enumerate(entries):
        resp = responses.get(str(i))
        if resp is None or not resp.get("ok"):
            print(
                f"oracle failed for entry {i} ({entry['id']}): {resp}", file=sys.stderr
            )
            return 1
        result = resp["result"]
        if result.get("accept"):
            golden["entries"].append(
                {"id": entry["id"], "accept": True, "canonical": result["canonical"]}
            )
        else:
            golden["entries"].append({"id": entry["id"], "accept": False})

    golden_path.write_text(
        json.dumps(golden, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
    )
    accepted = sum(1 for e in golden["entries"] if e["accept"])
    print(
        f"extracted {len(entries)} entries ({accepted} accept, "
        f"{len(entries) - accepted} reject; captured against {paper}) -> {golden_path}"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
