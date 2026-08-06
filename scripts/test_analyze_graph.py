#!/usr/bin/env python3
"""Regression tests for scripts/analyze_graph.py's mc.network class-cluster split.

Run with: python3 scripts/test_analyze_graph.py

Proves, against the real Paper tree under working/. analyze_graph.py hard-exits
if the Paper source roots are absent (it needs imports/LOC/cycle data from the
Java), so this suite requires the same working/ setup as the merge gate:

  1. `--split-network` and `--split-game` regeneration are byte-idempotent: two
     consecutive runs produce identical output (with carry applied between them).
  2. The baseline `--split-nbt --split-network --split-game` path still
     reproduces the committed MANIFEST.tsv byte-for-byte, so the nbt split, the
     network split and the package scan are untouched.
  3. The full Java inventory is conserved by the network split:
     - the net.minecraft.network package splits into exactly mc.network,
       mc.network.buf, mc.network.framing with the required file lists;
     - no file is lost or duplicated across the split (residual is the
       complement of the authored buf/framing file lists within the package);
     - the union of java_paths over the whole split manifest equals the union
       over the pre-split manifest (nothing gained or dropped anywhere).
  3b. The net.minecraft.network.protocol.game package splits into exactly
      mc.network.protocol.game (residual), .join, .chunk and .serverbound with
      the required file lists, conserving the 194-file / 11,497-LOC package.
  4. wave/cycle metadata is preserved: all split units keep the package's
     wave and cycle (they remain inside the giant SCC).
  5. status/attempts/notes carry across regeneration, including on the split
     units (so the later protocol PR's status transitions survive a rerun).
  6. every dep token in the split manifest resolves to a unit via the
     wave-picker's rules (exact unit id, derived package id, or package match).
"""

import csv
import subprocess
import sys
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
ANALYZE = REPO / "scripts" / "analyze_graph.py"
MANIFEST = REPO / "MANIFEST.tsv"

NETWORK_PKG = "net.minecraft.network"
BUF_FILES = {
    "VarInt.java", "VarLong.java", "Utf8String.java", "FriendlyByteBuf.java",
}
FRAMING_FILES = {"Varint21FrameDecoder.java", "Varint21LengthFieldPrepender.java"}
AUTHORED_FILES = BUF_FILES | FRAMING_FILES

GAME_PKG = "net.minecraft.network.protocol.game"
GAME_JOIN_FILES = {
    "ClientboundLoginPacket.java", "CommonPlayerSpawnInfo.java",
    "ClientboundChangeDifficultyPacket.java",
    "ClientboundPlayerAbilitiesPacket.java",
    "ClientboundSetHeldSlotPacket.java",
    "ClientboundUpdateRecipesPacket.java",
    "ClientboundInitializeBorderPacket.java",
    "ClientboundSetDefaultSpawnPositionPacket.java",
    "ClientboundSetTimePacket.java", "ClientboundGameEventPacket.java",
    "ClientboundPlayerInfoUpdatePacket.java",
    "ClientboundPlayerInfoRemovePacket.java",
    "ClientboundBundlePacket.java", "ClientboundBundleDelimiterPacket.java",
    "ClientboundPlayerPositionPacket.java",
}
GAME_CHUNK_FILES = {
    "ClientboundLevelChunkWithLightPacket.java",
    "ClientboundLevelChunkPacketData.java",
    "ClientboundLightUpdatePacket.java",
    "ClientboundLightUpdatePacketData.java",
    "ClientboundChunkBatchStartPacket.java",
    "ClientboundChunkBatchFinishedPacket.java",
}
GAME_SERVERBOUND_FILES = {
    "ServerboundMovePlayerPacket.java",
    "ServerboundChunkBatchReceivedPacket.java",
    "ServerboundAcceptTeleportationPacket.java",
    "ServerboundClientCommandPacket.java",
    "ServerboundClientTickEndPacket.java",
    "ServerboundPlayerActionPacket.java",
}
GAME_AUTHORED_FILES = GAME_JOIN_FILES | GAME_CHUNK_FILES | GAME_SERVERBOUND_FILES

PASS = 0
FAIL = 0


def check(name: str, cond: bool, detail: str = "") -> None:
    global PASS, FAIL
    if cond:
        PASS += 1
        print(f"  ok  {name}")
    else:
        FAIL += 1
        print(f"FAIL  {name}" + (f" — {detail}" if detail else ""))


def rows_of(path: Path) -> list[dict[str, str]]:
    with path.open(newline="", encoding="utf-8") as fh:
        return list(csv.DictReader(fh, delimiter="\t"))


def run_analyze(*flags: str, out: Path, prev: Path) -> None:
    subprocess.run(
        [sys.executable, str(ANALYZE), *flags, "--output", str(out),
         "--prev-manifest", str(prev)],
        check=True,
        capture_output=True,
        text=True,
    )


def derive_id(pkg: str) -> str:
    return (
        pkg.replace("net.minecraft.", "mc.")
        .replace("org.bukkit.", "bukkit.")
        .replace("io.papermc.paper.", "paper.")
    )


def resolve_dep(token: str, by_id: dict[str, dict],
                by_pkg: dict[str, list[dict]]) -> dict | None:
    if token in by_id:
        return by_id[token]
    derived = derive_id(token)
    if derived in by_id:
        return by_id[derived]
    matches = by_pkg.get(token, [])
    if matches:
        return matches[0]
    return None


def main() -> None:
    if not MANIFEST.exists():
        print("MANIFEST.tsv missing; cannot run regression tests")
        return 2

    with tempfile.TemporaryDirectory(prefix="rivet-analyze-test-") as tmp:
        tmpd = Path(tmp)
        # ---- 2. baseline: the committed manifest is reproducible byte-for-byte --
        base = tmpd / "base.tsv"
        run_analyze("--split-nbt", "--split-network", "--split-game",
                    out=base, prev=MANIFEST)
        check("baseline: all splits reproduce committed MANIFEST.tsv",
              base.read_bytes() == MANIFEST.read_bytes())

        # ---- 1. idempotency of the network split --------------------------------
        net1 = tmpd / "net1.tsv"
        net2 = tmpd / "net2.tsv"
        run_analyze("--split-network", out=net1, prev=MANIFEST)
        run_analyze("--split-network", out=net2, prev=net1)
        check("network split: two regenerations byte-identical",
              net1.read_bytes() == net2.read_bytes())

        split = rows_of(net1)
        by_id = {r["id"]: r for r in split}
        by_pkg: dict[str, list[dict]] = {}
        for r in split:
            by_pkg.setdefault(r["java_package"], []).append(r)

        # ---- 3. inventory conservation ------------------------------------------
        network_units = [r for r in split if r["java_package"] == NETWORK_PKG]
        check("network split: exactly 3 units",
              sorted(r["id"] for r in network_units)
              == ["mc.network", "mc.network.buf", "mc.network.framing"])
        check("network split: source_root preserved (minecraft)",
              all(r["source_root"] == "minecraft" for r in network_units))

        # The residual mc.network unit stays needs_split=yes: it still owns 36
        # files (over SPLIT_FILE_THRESHOLD) and is cyclic, so it must not be
        # pickable as if the split were complete. buf/framing are the M1
        # protocol wave's deliverable and must not be flagged.
        check("residual mc.network keeps needs_split=yes",
              by_id["mc.network"]["needs_split"] == "yes")
        check("buf/framing are not needs_split",
              by_id["mc.network.buf"]["needs_split"] == ""
              and by_id["mc.network.framing"]["needs_split"] == "")

        # Same-package references from buf/framing into the residual are
        # deliberately NOT dep edges (the residual is not translated in M1, so
        # recording them would deadlock the wave); the delivered modules model
        # the residual touchpoints themselves (BandwidthDebugMonitor as an fn
        # callback, ADVENTURE_LOCALE absent), so no STUB note is authored.

        def file_set(r: dict) -> set[str]:
            return {p.rsplit("/", 1)[-1] for p in r["java_paths"].split(",")}

        buf_set = file_set(by_id["mc.network.buf"])
        framing_set = file_set(by_id["mc.network.framing"])
        residual_set = file_set(by_id["mc.network"])
        check("buf unit owns exactly VarInt/VarLong/Utf8String/FriendlyByteBuf",
              buf_set == BUF_FILES)
        check("framing unit owns exactly the varint21 frame codec pair",
              framing_set == FRAMING_FILES)
        check("residual owns the complement (no authored file in it)",
              residual_set.isdisjoint(AUTHORED_FILES))
        # Every java file actually under the network package dir is owned exactly
        # once across the three units (checked against disk when working/ is
        # present; the manifest-based conservation checks above still hold then).
        pkg_dir = (REPO / "working/Paper/paper-server/src/minecraft/java"
                   / "net/minecraft/network")
        if pkg_dir.is_dir():
            owned = set()
            dup = set()
            for r in network_units:
                for f in file_set(r):
                    (dup if f in owned else owned).add(f)
            check("network package: every on-disk *.java owned exactly once",
                  owned == {p.name for p in pkg_dir.glob("*.java")} and not dup,
                  f"owned={len(owned)} dup={sorted(dup)}")

        base = rows_of(MANIFEST)
        base_paths = {p for r in base for p in r["java_paths"].split(",")}
        split_paths = {p for r in split for p in r["java_paths"].split(",")}
        check("whole manifest: Java inventory conserved (no loss, no duplication)",
              base_paths == split_paths,
              f"base={len(base_paths)} split={len(split_paths)}")
        check("network split: file counts conserved",
              sum(int(r["files"]) for r in network_units)
              == sum(int(r["files"]) for r in base if r["java_package"] == NETWORK_PKG))
        check("network split: LOC conserved",
              sum(int(r["loc"]) for r in network_units)
              == sum(int(r["loc"]) for r in base if r["java_package"] == NETWORK_PKG))

        # ---- 3b. game split: join-critical sub-units (#152) ----------------------
        game1 = tmpd / "game1.tsv"
        game2 = tmpd / "game2.tsv"
        run_analyze("--split-game", out=game1, prev=MANIFEST)
        run_analyze("--split-game", out=game2, prev=game1)
        check("game split: two regenerations byte-identical",
              game1.read_bytes() == game2.read_bytes())
        gsplit = rows_of(game1)
        g_by_id = {r["id"]: r for r in gsplit}
        g_by_pkg: dict[str, list[dict]] = {}
        for r in gsplit:
            g_by_pkg.setdefault(r["java_package"], []).append(r)
        game_units = [r for r in gsplit if r["java_package"] == GAME_PKG]
        check("game split: exactly 4 units",
              sorted(r["id"] for r in game_units)
              == ["mc.network.protocol.game", "mc.network.protocol.game.chunk",
                  "mc.network.protocol.game.join",
                  "mc.network.protocol.game.serverbound"])
        check("game split: source_root preserved (minecraft)",
              all(r["source_root"] == "minecraft" for r in game_units))

        # The residual mc.network.protocol.game stays needs_split=yes: it still
        # owns 167 files (over SPLIT_FILE_THRESHOLD) and is cyclic, so it must
        # not be pickable as if the split were complete. The three join-critical
        # sub-units are the M1 protocol wave's deliverable and must not be flagged.
        check("residual game keeps needs_split=yes",
              g_by_id["mc.network.protocol.game"]["needs_split"] == "yes")
        check("game sub-units are not needs_split",
              all(g_by_id[u]["needs_split"] == "" for u in
                  ("mc.network.protocol.game.join", "mc.network.protocol.game.chunk",
                   "mc.network.protocol.game.serverbound")))

        join_set = file_set(g_by_id["mc.network.protocol.game.join"])
        chunk_set = file_set(g_by_id["mc.network.protocol.game.chunk"])
        sb_set = file_set(g_by_id["mc.network.protocol.game.serverbound"])
        game_residual_set = file_set(g_by_id["mc.network.protocol.game"])
        check("game.join owns the #87 join clientbound send-set",
              join_set == GAME_JOIN_FILES)
        check("game.chunk owns the #94 chunk-send packet bodies",
              chunk_set == GAME_CHUNK_FILES)
        check("game.serverbound owns the #97 serverbound play essentials",
              sb_set == GAME_SERVERBOUND_FILES)
        check("game residual owns the complement (no authored file in it)",
              game_residual_set.isdisjoint(GAME_AUTHORED_FILES))
        # Every java file actually under the game package dir is owned exactly
        # once across the four units (checked against disk when working/ is
        # present; the manifest-based conservation checks above still hold then).
        game_dir = (REPO / "working/Paper/paper-server/src/minecraft/java"
                    / "net/minecraft/network/protocol/game")
        if game_dir.is_dir():
            owned = set()
            dup = set()
            for r in game_units:
                for f in file_set(r):
                    (dup if f in owned else owned).add(f)
            check("game package: every on-disk *.java owned exactly once",
                  owned == {p.name for p in game_dir.glob("*.java")} and not dup,
                  f"owned={len(owned)} dup={sorted(dup)}")
        base_game = [r for r in base if r["java_package"] == GAME_PKG]
        check("game split: file counts conserved",
              sum(int(r["files"]) for r in game_units)
              == sum(int(r["files"]) for r in base_game))
        check("game split: LOC conserved",
              sum(int(r["loc"]) for r in game_units)
              == sum(int(r["loc"]) for r in base_game))
        check("game split: wave preserved",
              all(r["wave"] == base_game[0]["wave"] for r in game_units))
        check("game split: cycle preserved",
              all(r["cycle"] == base_game[0]["cycle"] for r in game_units))

        # ---- 4. wave/cycle metadata preserved ------------------------------------
        base_nw = [r for r in base if r["java_package"] == NETWORK_PKG]
        check("network split: wave preserved",
              all(r["wave"] == base_nw[0]["wave"] for r in network_units))
        check("network split: cycle preserved",
              all(r["cycle"] == base_nw[0]["cycle"] for r in network_units))

        # ---- 6. every dep resolves via the wave-picker rules ---------------------
        unresolved = []
        for r in split:
            for tok in (t.strip() for t in r["deps"].split(",") if t.strip()):
                if resolve_dep(tok, by_id, by_pkg) is None:
                    unresolved.append((r["id"], tok))
        check("all dep tokens in the split manifest resolve to a unit",
              not unresolved, unresolved[:5])

        g_unresolved = []
        for r in gsplit:
            for tok in (t.strip() for t in r["deps"].split(",") if t.strip()):
                if resolve_dep(tok, g_by_id, g_by_pkg) is None:
                    g_unresolved.append((r["id"], tok))
        check("all dep tokens in the game split manifest resolve to a unit",
              not g_unresolved, g_unresolved[:5])

        # ---- 5. status/attempts/notes carry across regeneration ------------------
        seeded = tmpd / "seeded.tsv"
        carry_rows = []
        for r in split:
            if r["id"] in ("mc.network", "mc.network.buf", "mc.network.framing"):
                r["status"] = "translated"
                r["attempts"] = "2"
                r["notes"] = "protocol-wave note"
            carry_rows.append(r)
        with seeded.open("w", encoding="utf-8") as fh:
            fh.write("\t".join(carry_rows[0].keys()) + "\n")
            for r in carry_rows:
                fh.write("\t".join(r.values()) + "\n")
        regen = tmpd / "regen.tsv"
        run_analyze("--split-network", out=regen, prev=seeded)
        regen_rows = rows_of(regen)
        # The seeded human note must survive regeneration (append-only, never
        # clobbering the human note).
        for unit_id in ("mc.network", "mc.network.buf", "mc.network.framing"):
            r = next(x for x in regen_rows if x["id"] == unit_id)
            check(f"carry: {unit_id} keeps status/attempts/notes",
                  r["status"] == "translated" and r["attempts"] == "2"
                  and "protocol-wave note" in r["notes"])

        # Game-split carry: seed the four game units, regenerate with --split-game,
        # and verify status/attempts/notes (incl. the authored STUB note appended
        # alongside the human note) survive.
        g_seeded = tmpd / "g_seeded.tsv"
        g_carry_rows = []
        for r in gsplit:
            if r["java_package"] == GAME_PKG:
                r["status"] = "translated"
                r["attempts"] = "2"
                r["notes"] = "game-wave note"
            g_carry_rows.append(r)
        with g_seeded.open("w", encoding="utf-8") as fh:
            fh.write("\t".join(g_carry_rows[0].keys()) + "\n")
            for r in g_carry_rows:
                fh.write("\t".join(r.values()) + "\n")
        g_regen = tmpd / "g_regen.tsv"
        run_analyze("--split-game", out=g_regen, prev=g_seeded)
        g_regen_rows = rows_of(g_regen)
        for unit_id in ("mc.network.protocol.game", "mc.network.protocol.game.join",
                        "mc.network.protocol.game.chunk",
                        "mc.network.protocol.game.serverbound"):
            r = next(x for x in g_regen_rows if x["id"] == unit_id)
            check(f"game carry: {unit_id} keeps status/attempts/notes",
                  r["status"] == "translated" and r["attempts"] == "2"
                  and "game-wave note" in r["notes"])
        # The authored STUB note must also be present (never clobbering the
        # human note) for the three sub-units.
        for unit_id in ("mc.network.protocol.game.join",
                        "mc.network.protocol.game.chunk",
                        "mc.network.protocol.game.serverbound"):
            r = next(x for x in g_regen_rows if x["id"] == unit_id)
            check(f"game carry: {unit_id} keeps authored STUB note",
                  "M1 STUB:" in r["notes"])

        # ---- all flags compose ---------------------------------------------------
        both = tmpd / "both.tsv"
        run_analyze("--split-nbt", "--split-network", "--split-game",
                    out=both, prev=MANIFEST)
        both_rows = rows_of(both)
        both_ids = {r["id"] for r in both_rows}
        check("all flags compose: nbt + network + game split units present",
              {"mc.nbt", "mc.nbt.snbt", "mc.network", "mc.network.buf",
               "mc.network.framing", "mc.network.protocol.game",
               "mc.network.protocol.game.join", "mc.network.protocol.game.chunk",
               "mc.network.protocol.game.serverbound"} <= both_ids)

        # ---- fail-fast on cross-unit duplicate declarations -----------------------
        # A file listed in two units would be double-counted and silently dropped
        # from the residual; the analyzer must refuse to emit rows. Simulate the
        # mistake by declaring FriendlyByteBuf.java in both buf and framing, and
        # pin REPO to the real tree so the temp copy finds the Paper sources.
        dup_src = ANALYZE.read_text(encoding="utf-8").replace(
            '"Varint21FrameDecoder.java", "Varint21LengthFieldPrepender.java",',
            '"Varint21FrameDecoder.java", "Varint21LengthFieldPrepender.java", "FriendlyByteBuf.java",',
        ).replace(
            "REPO = Path(__file__).resolve().parent.parent",
            f"REPO = Path({str(REPO)!r})",
        )
        dup_script = tmpd / "analyze_graph_dup.py"
        dup_script.write_text(dup_src, encoding="utf-8")
        dup_proc = subprocess.run(
            [sys.executable, str(dup_script), "--split-network",
             "--output", str(tmpd / "dup.tsv"), "--prev-manifest", str(MANIFEST)],
            capture_output=True, text=True,
        )
        check("duplicate declaration exits nonzero (fail-fast)",
              dup_proc.returncode != 0)
        check("duplicate declaration names both owning units",
              "FriendlyByteBuf.java is declared in both mc.network.buf "
              "and mc.network.framing" in dup_proc.stderr)

        # Game-split fail-fast: declare ClientboundGameEventPacket.java in both
        # the join and serverbound units and require a nonzero exit naming both
        # owning units.
        gdup_src = ANALYZE.read_text(encoding="utf-8").replace(
            '        "ServerboundMovePlayerPacket.java",\n'
            '        "ServerboundChunkBatchReceivedPacket.java",',
            '        "ServerboundMovePlayerPacket.java",\n'
            '        "ServerboundChunkBatchReceivedPacket.java",\n'
            '        "ClientboundGameEventPacket.java",',
        ).replace(
            "REPO = Path(__file__).resolve().parent.parent",
            f"REPO = Path({str(REPO)!r})",
        )
        gdup_script = tmpd / "analyze_graph_gdup.py"
        gdup_script.write_text(gdup_src, encoding="utf-8")
        gdup_proc = subprocess.run(
            [sys.executable, str(gdup_script), "--split-game",
             "--output", str(tmpd / "gdup.tsv"), "--prev-manifest", str(MANIFEST)],
            capture_output=True, text=True,
        )
        check("game duplicate declaration exits nonzero (fail-fast)",
              gdup_proc.returncode != 0)
        check("game duplicate declaration names both owning units",
              "ClientboundGameEventPacket.java is declared in both "
              "mc.network.protocol.game.join and "
              "mc.network.protocol.game.serverbound" in gdup_proc.stderr)

        # ---- plain (no flags) is idempotent ---------------------------------------
        plain1 = tmpd / "plain1.tsv"
        plain2 = tmpd / "plain2.tsv"
        run_analyze(out=plain1, prev=MANIFEST)
        run_analyze(out=plain2, prev=plain1)
        check("no-split regeneration byte-idempotent",
              plain1.read_bytes() == plain2.read_bytes())

    print(f"\n{PASS} passed, {FAIL} failed")
    return 1 if FAIL else 0


if __name__ == "__main__":
    sys.exit(main())
