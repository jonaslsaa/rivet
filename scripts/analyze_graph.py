#!/usr/bin/env python3
"""Builds MANIFEST.tsv (package-level units) from the Paper sources in working/Paper.

Parses package declarations and imports from every Java file, groups them into
package-level units, maps packages to target crates, records inter-package
dependencies, condenses cycles (Tarjan SCC), and assigns each unit a topological
`wave` number: a unit is safe to translate once every unit with a lower wave in
its dependency closure is done. Units in the same SCC share a `cycle` id and must
be scheduled together (or split first — see the needs_split flag).

`--split-nbt` refines the net.minecraft.nbt package into class-cluster units
(epic #9): one irreducible SCC (the sealed Tag hierarchy + visitor interfaces +
type system + accounter + exceptions, which map to Rust enums and cannot split
without stubs) plus the downstream layers (io, ops, snbt, text, utils,
visitors). The split used to live in scripts/split_nbt_units.py; it is folded
here so re-running the analyzer is idempotent with the split.

`--split-network` refines the net.minecraft.network package (issue #65, M1): the
existing mc.network row is split into mc.network.buf (VarInt, VarLong, Utf8String,
FriendlyByteBuf), mc.network.framing (Varint21FrameDecoder,
Varint21LengthFieldPrepender) and a residual mc.network unit holding every
remaining file of the package. The residual is computed as the complement of the
buf/framing file lists within the package scan, so the split can never lose or
duplicate a file. Deps are computed per file from each file's imports (the nbt
split hand-authors deps because its class clusters are authored; the network
package's external deps are ordinary imports and are read straight from source),
with only the same-package sibling edges authored as unit ids (buf <- framing <-
residual). All three units keep the package's wave and cycle: they remain inside
the giant SCC (FriendlyByteBuf <-> net.minecraft.network.codec is a class-level
back-edge), so the split right-sizes file ownership without claiming the units
are cycle-free. The residual unit stays flagged needs_split=yes (it still owns
36 files and is not done); the small buf/framing cluster units are the M1
protocol wave's deliverable and are not flagged.

`--split-game` refines the net.minecraft.network.protocol.game package (issue
#152, M1): the single 194-file / 11,497-LOC game row is split into the
join-critical sub-units that unblock the three M1 protocol tracks —
mc.network.protocol.game.join (#87 join clientbound send-set),
mc.network.protocol.game.chunk (#94 chunk send packet bodies) and
mc.network.protocol.game.serverbound (#97 serverbound play essentials) — plus a
residual mc.network.protocol.game unit computed as the complement of the
authored sub-unit file lists within the package scan, so the split can never
lose or duplicate a file. Same mechanics as the network split: sub-unit deps are
read per file from each file's imports, the four units keep the package's
wave/cycle (they remain inside the giant SCC), the residual depends on the three
sub-units (same-package classes need no import: GameProtocols/GamePacketTypes/
the listeners all reference the sub-unit packets), and the sub-units' references
back into the residual (ClientGamePacketListener, ServerGamePacketListener,
GamePacketTypes) are deliberately NOT recorded — the residual is not translated
in M1, so recording them would deadlock the wave; the M1 translate-wave absorbs
those residual classes as STUBs (see SPLIT_NOTES). The residual keeps
needs_split=yes (still 167 files and not done); the three sub-units are the M1
protocol wave's deliverable and are not flagged.

`--split-server` refines the oversized net.minecraft.server.level package in
scope of issue #227 (M2, prerequisite for #185): the 37-file / 13,246-LOC
monolithic mc.server.level row splits into right-sized class clusters around
the eight named pipeline targets — ServerChunkCache, ChunkMap, ChunkHolder,
ChunkLevel, ChunkTaskDispatcher, ChunkTracker, ChunkTrackingView,
ThreadedLevelLightEngine — plus a ticket value layer (Ticket/TicketType), a
worldgen view (WorldGenRegion) and a residual hub. The residual is computed as
the complement of the authored file lists within the package scan, so the split
can never lose or duplicate a file; it keeps the pre-split id (mc.server.level)
because 200+ external rows depend on net.minecraft.server.level and must
resolve to a single hub, not to the lowest-id cluster. All units keep the
package's wave/cycle (the whole package is one class-level SCC inside cycle
27), so the split right-sizes *file ownership* — it does not claim the units
are acyclic. Cross-cluster same-package edges are authored as unit ids in
SPLIT_EDGES (Java same-package refs need no import); every sub-unit's
back-references into the residual (ServerLevel/ServerPlayer/etc.) are
deliberate STUBs recorded in SPLIT_NOTES, not dep edges (recording them would
deadlock the wave). The intra-cluster DAG (all verified against the Java):
level -> holder -> task -> light and holder -> tracker -> distance -> chunkmap
-> light/servercache, with view -> chunkmap and region on holder; the residual
depends on chunkmap/holder/servercache/ticket/view.

`--split-world` refines the oversized mc.world.level.* packages in scope of
issue #176 (M2): the levelgen root, levelgen.feature, feature.configurations,
levelgen.blockpredicates, levelgen.placement, biome, levelgen.structure,
structure.structures, structure.templatesystem, chunk, storage and lighting
become right-sized class-cluster units. Fully partitioned (every file lands in
an authored cluster, so the pre-split package row disappears and external deps
on the package resolve to the lowest-id cluster via the wave-picker's
shared-java_package rule): levelgen root (random -> noise -> noisegen ->
settings/surface/spawner), biome (core/source/data), feature (core + 8 family
clusters + 39 independent leaf units — 40 leaf files; the fossil unit bundles
FossilFeature + FossilFeatureConfiguration), feature.configurations (core + 37
leaf configs), blockpredicates (core + combinators/states/simple), placement
(core + repeating/filter/simple), chunk (wire/access/support/generator),
templatesystem (core/rules/processors) and lighting (core/engine). The other
split packages (structure, structures, storage) keep a residual tail computed
as the complement of the authored file lists within the package scan, exactly
like the network/game splits. All units keep the package's wave/cycle (they
remain inside the giant SCC), so the split right-sizes *file ownership* — it
does not claim the units are acyclic. Cross-unit same-package references are
class-level edges the package graph cannot see (Java same-package refs need no
import); they are recorded as unit-id deps in SPLIT_EDGES (residual ->
sub-units, the levelgen/biome/feature/blockpredicates/placement/chunk/
templatesystem/lighting intra-cluster DAGs), and the deliberate back-edges that
would deadlock the wave (noisegen -> surface/settings, feature core -> leaves,
templatesystem processors -> core) live in SPLIT_NOTES as STUB notes instead.
The 39 feature leaf units and 37 config leaves are genuinely independent — each references only
feature.core / configurations.core (verified against the Java: no leaf names a
sibling leaf except the fossil pair bundling FossilFeature +
FossilFeatureConfiguration, and two config pairs) — so the #181 feature wave can
pick them in parallel once the core hub is done. blockpredicates and placement
are NOT irreducible (the earlier claim was wrong): each is a hub-and-spoke star
on a register() registry (BlockPredicateType / PlacementModifierType) plus
interface factories, so they split exactly like the feature package.

The chunk split is a strict DAG with no STUBs: wire (the palette-container
value layer, aligned with the M1 #108 scaffold) and support (the heightmap/
light/carving/block-column leaf types) are dependency-free leaves, access (the
#183 in-memory chunk data SCC + ChunkSource provider seam) builds on both
(UpgradeData rides in access: chunk-NBT upgrade operates on LevelChunk/
LevelChunkSection), and generator (the #185 pipeline-facing generator stack)
builds on access — verified against the Java: zero wire -> non-wire and zero
support -> access edges. The storage split keeps a residual for the same reason
the network/game splits do: LevelStorageSource is the on-disk access hub (region
/level.dat/playerdata), and keeping the residual id (derive_id) means the 100+
rows that depend on net.minecraft.world.level.storage resolve to it rather than
to the lowest-id cluster (leveldata, which owns only the LevelData interface
family). The storage foundation clusters are independent leaves; the residual
references leveldata/value/version + primaryleveldata (saveddata is untouched —
verified zero saveddata refs); primaryleveldata builds on leveldata + version;
and there are zero sub-unit -> residual back-edges (uniquely clean — no STUB
notes).

The network, game, world and server splits share one generic
package_split_rows(pkg) below; the four opt-in flags (--split-network,
--split-game, --split-world, --split-server) plus --split-nbt are all
additive, and pass none to get the flat package-level manifest. The default
output and previous-manifest locations can be overridden with `--output` and
`--prev-manifest` (used by the regression tests).

`needs_split` is the *actionable* pre-translation split state: `yes` iff the
unit is not done and owns more than SPLIT_FILE_THRESHOLD files. Structural SCC
pressure lives in the separate `cycle` column (a non-empty cycle id means the
unit is a member of an SCC that must move together) and in `files`/`loc` — these
are graph facts that persist even for done units. `needs_split` is deliberately
the derived boolean the wave-picker gates on (unless --include-needs-split),
so a completed unit never advertises itself as a pre-translation splitting
candidate: it loses the flag on the next regeneration regardless of graph shape,
while its `cycle` membership (if any) stays visible. The split rows below
follow the same rule, gated on their own status.

Every unit also gets a `java_paths` column (comma-joined `root:relpath`
identifiers) and a `source_root` column naming the source roots those files live
under. Each `java_paths` entry is prefixed with the source root the file was
found under (`minecraft:`, `paper-server:` or `paper-api:`), so a physical file
is identified unambiguously even when the same relative path exists under two
roots (e.g. io.papermc.paper code under both paper-api and paper-server, or
moonrise code under both minecraft/java and main/java). `source_root` is the
comma-joined list of the distinct roots the unit's files span. The root prefix
is the unit of ownership: every physical file must appear exactly once across
the whole manifest, which main() validates against the on-disk inventory. This
gives each split unit its concrete file set, so a unit's dep on a package it
shares (net.minecraft.nbt) is a real dep on the sibling unit that owns those
files — never a self-dependency. Cross-unit deps within a shared package are
authored as unit ids (e.g. `mc.nbt.snbt`), which the wave-picker resolves by
exact id.

The nbt core is *not* cycle-free: the Tag classes' `toString()` uses
`StringTagVisitor` (snbt) and `StringTag` uses `SnbtGrammar` (snbt), while
`CompoundTag` uses `NbtOps.INSTANCE` (ops). So mc.nbt, mc.nbt.snbt and mc.nbt.ops
form a class-level SCC (cycle id `nbt`) that must be scheduled together; the
back-edges are recorded in each unit's notes column.

Known limitation: same-package class references need no import in Java, so the
package-level graph cannot see intra-package edges; the nbt split's boundaries,
deps and cycle are therefore authored data (NBT_UNITS below), validated against
disk.

Re-running the analyzer preserves each unit's durable `status`/`attempts`/`notes`
(by id) from the existing MANIFEST.tsv, so regeneration never resets ported work.
"""

import argparse
import csv
import re
import sys
from collections import defaultdict
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
ROOTS = {
    "minecraft": REPO / "working/Paper/paper-server/src/minecraft/java",
    "paper-server": REPO / "working/Paper/paper-server/src/main/java",
    "paper-api": REPO / "working/Paper/paper-api/src/main/java",
}

CRATE_RULES = [
    ("com.mojang.brigadier", "rivet-brigadier"),
    ("com.mojang.serialization", "rivet-serialization"),
    ("com.mojang.datafixers", "rivet-serialization"),
    ("net.minecraft.nbt", "rivet-nbt"),
    ("net.minecraft.network.chat", "rivet-text"),
    ("net.minecraft.network", "rivet-protocol"),
    ("net.minecraft.util", "rivet-util"),
    ("net.minecraft.core", "rivet-registry"),
    ("net.minecraft.resources", "rivet-registry"),
    ("net.minecraft.tags", "rivet-registry"),
    ("net.minecraft.data", "rivet-registry"),
    ("net.minecraft.world.entity", "rivet-entity"),
    ("net.minecraft.world.level", "rivet-world"),
    ("net.minecraft.world", "rivet-world"),
    ("net.minecraft.gametest", "rivet-oracle"),
    ("net.minecraft.server", "rivet-server"),
    ("net.minecraft.commands", "rivet-server"),
    ("net.minecraft", "rivet-server"),
    ("org.bukkit.craftbukkit", "rivet-server"),
    ("io.papermc.paper", "rivet-api"),
    ("org.bukkit", "rivet-api"),
    ("org.spigotmc", "rivet-server"),
    ("com.destroystokyo.paper", "rivet-api"),
]

PKG_RE = re.compile(r"^\s*package\s+([\w.]+)\s*;", re.M)
IMP_RE = re.compile(r"^\s*import\s+(?:static\s+)?([\w.]+)\s*;", re.M)
INTERNAL_PREFIXES = tuple(p for p, _ in CRATE_RULES)
SPLIT_FILE_THRESHOLD = 15

# Class-cluster split of net.minecraft.nbt (epic #9): unit id -> metadata.
# (java_package, [file names relative to the nbt package dir], wave, cycle,
#  [deps], notes).
# mc.nbt, mc.nbt.snbt and mc.nbt.ops are one class-level SCC (cycle id `nbt`):
# the Tag classes' toString() -> StringTagVisitor, StringTag -> SnbtGrammar,
# CompoundTag -> NbtOps.INSTANCE, and TagParser -> NbtOps. They share a wave and
# must be scheduled together (wave-picker --include-cycles). Intra-cycle deps are
# NOT listed (a dep on `net.minecraft.nbt` would self-cycle); back-edges live in
# notes. Every other downstream unit depends on the core (`net.minecraft.nbt`,
# resolved to `mc.nbt` by the wave-picker) plus its own externals. Deps are java
# packages except where a unit must name a sibling unit directly (unit ids such
# as `mc.nbt.snbt`); both forms are understood by the wave-picker.
NBT_UNITS = {
    "mc.nbt": (
        "net.minecraft.nbt",
        [
            "Tag.java", "CollectionTag.java", "PrimitiveTag.java", "NumericTag.java",
            "EndTag.java", "ByteTag.java", "ShortTag.java", "IntTag.java",
            "LongTag.java", "FloatTag.java", "DoubleTag.java", "StringTag.java",
            "ByteArrayTag.java", "IntArrayTag.java", "LongArrayTag.java",
            "ListTag.java", "CompoundTag.java", "TagVisitor.java",
            "StreamTagVisitor.java", "TagType.java", "TagTypes.java",
            "NbtAccounter.java", "NbtException.java", "NbtAccounterException.java",
            "NbtFormatException.java", "ReportedNbtException.java",
            "package-info.java",
        ],
        3, "nbt",
        ["com.mojang.serialization", "net.minecraft", "net.minecraft.util"],
        "cycle nbt (mc.nbt/snbt/ops): Tag.toString -> StringTagVisitor (snbt), "
        "StringTag -> SnbtGrammar (snbt), CompoundTag -> NbtOps.INSTANCE (ops)",
    ),
    "mc.nbt.io": (
        "net.minecraft.nbt",
        ["NbtIo.java"],
        4, "",
        ["net.minecraft.nbt", "net.minecraft", "net.minecraft.util"],
        "",
    ),
    "mc.nbt.ops": (
        "net.minecraft.nbt",
        ["NbtOps.java"],
        3, "nbt",
        ["com.mojang.datafixers.util", "com.mojang.serialization", "net.minecraft.util"],
        "cycle nbt (mc.nbt/snbt/ops): NbtOps is DynamicOps<Tag> over core; "
        "core CompoundTag -> NbtOps.INSTANCE; TagParser (snbt) -> NbtOps",
    ),
    "mc.nbt.snbt": (
        "net.minecraft.nbt",
        ["SnbtGrammar.java", "SnbtOperations.java", "TagParser.java",
         "StringTagVisitor.java", "SnbtPrinterTagVisitor.java"],
        3, "nbt",
        ["com.mojang.brigadier", "com.mojang.brigadier.exceptions",
         "com.mojang.serialization", "net.minecraft.core", "net.minecraft.network.chat",
         "net.minecraft.util", "net.minecraft.util.parsing.packrat",
         "net.minecraft.util.parsing.packrat.commands"],
        "cycle nbt (mc.nbt/snbt/ops): core Tag.toString -> StringTagVisitor, "
        "StringTag -> SnbtGrammar; TagParser -> NbtOps (ops)",
    ),
    "mc.nbt.text": (
        "net.minecraft.nbt",
        ["TextComponentTagVisitor.java"],
        4, "",
        ["net.minecraft.nbt", "net.minecraft", "net.minecraft.network.chat"],
        "",
    ),
    "mc.nbt.utils": (
        "net.minecraft.nbt",
        ["NbtUtils.java"],
        5, "",
        ["mc.nbt.snbt", "mc.nbt.text", "net.minecraft.nbt",
         "com.mojang.brigadier.exceptions", "com.mojang.serialization",
         "net.minecraft", "net.minecraft.core", "net.minecraft.core.registries",
         "net.minecraft.network.chat", "net.minecraft.resources", "net.minecraft.util",
         "net.minecraft.world.level.block", "net.minecraft.world.level.block.state",
         "net.minecraft.world.level.block.state.properties", "net.minecraft.world.level.material",
         "net.minecraft.world.level.storage"],
        "uses snbt (SnbtPrinterTagVisitor, TagParser) and text (TextComponentTagVisitor); "
        "those are unit-id deps (shared java_package)",
    ),
    "mc.nbt.visitors": (
        "net.minecraft.nbt.visitors",
        ["visitors/CollectFields.java", "visitors/CollectToTag.java",
         "visitors/FieldSelector.java", "visitors/FieldTree.java",
         "visitors/SkipAll.java", "visitors/SkipFields.java",
         "visitors/package-info.java"],
        4, "",
        ["net.minecraft.nbt"],
        "",
    ),
}
NBT_SPLIT_PACKAGES = {"net.minecraft.nbt", "net.minecraft.nbt.visitors"}
NBT_DIR = Path("net/minecraft/nbt")

# Class-cluster split of net.minecraft.network (issue #65, M1): unit id -> file
# names (relative to the network package dir). mc.network.buf and mc.network.framing
# are the byte-buffer/varint codecs and the varint21 frame codec the first protocol
# wave needs; the residual mc.network unit is computed as the complement of the
# package scan (never hand-enumerated), so the split cannot lose or duplicate a
# file. Unlike the nbt split, only the file lists and the same-package sibling
# edges are authored here: each unit's external deps are read from its own files'
# imports (see file_deps), and all three units keep the package's wave/cycle — they
# remain inside the giant SCC (FriendlyByteBuf <-> net.minecraft.network.codec is a
# class-level back-edge), so this right-sizes file ownership, it does not claim the
# units are cycle-free. The residual mc.network unit stays needs_split=yes (still
# oversized and cyclic); buf/framing are the M1 protocol wave's deliverable and are
# not flagged.
#
# Same-package references from buf/framing back into the residual are NOT dep
# edges (see the same-package note in unit_row below): the residual is not
# translated in M1, so recording them would deadlock the wave. The delivered
# modules model the residual touchpoints themselves (Varint21FrameDecoder takes
# an optional BandwidthDebugMonitor fn callback; FriendlyByteBuf's
# ADVENTURE_LOCALE/registry paths are simply absent — see module docs).
# Class-cluster splits, keyed by java package: sub_unit_id -> file names
# relative to the package dir. package_split_rows() below turns each entry into
# authored sub-unit rows plus a residual row whose id is derive_id(pkg) (== the
# pre-split row's id, so carry maps the old row's status/notes onto it). The
# residual is always the complement of the authored file lists within the
# package scan — never hand-enumerated — so the split can never lose or
# duplicate a file. Sub-unit deps are read per file from each file's imports;
# only same-package sibling edges are authored, as unit ids in SPLIT_EDGES
# (Java same-package refs need no import, so the package graph cannot see them).
# All units keep the package's wave/cycle: the split right-sizes file ownership,
# it does not claim the units are cycle-free.
PACKAGE_SPLITS: dict[str, dict[str, list[str]]] = {
    # ---- net.minecraft.network (issue #65, M1): mc.network.buf (the
    # byte-buffer/varint codecs), mc.network.framing (the varint21 frame codec
    # pair) + a residual mc.network holding every other file of the package. The
    # residual depends on buf and framing (same-package classes need no import);
    # buf/framing in turn reference residual classes (FriendlyByteBuf reads
    # PacketEncoder.ADVENTURE_LOCALE; Varint21FrameDecoder takes an optional
    # BandwidthDebugMonitor callback) but those reverse edges are NOT recorded:
    # the residual is not translated in M1, so recording them would deadlock the
    # wave — the delivered modules model the residual touchpoints themselves.
    "net.minecraft.network": {
        "mc.network.buf": [
            "VarInt.java", "VarLong.java", "Utf8String.java", "FriendlyByteBuf.java",
        ],
        "mc.network.framing": [
            "Varint21FrameDecoder.java", "Varint21LengthFieldPrepender.java",
        ],
    },
    # ---- net.minecraft.network.protocol.game (issue #152, M1): the
    # join-critical sub-units that unblock the three M1 protocol tracks (#87
    # join clientbound send-set, #94 chunk send, #97 serverbound play essentials)
    # + a residual computed as the complement of the authored lists. The residual
    # depends on the three sub-units (GameProtocols/GamePacketTypes/the
    # listeners all reference the sub-unit packets); the sub-units' references
    # back into the residual (ClientGamePacketListener, ServerGamePacketListener,
    # GamePacketTypes) are deliberately NOT recorded — the residual is not
    # translated in M1, and the M1 translate-wave absorbs those residual classes
    # as STUBs instead (see SPLIT_NOTES).
    "net.minecraft.network.protocol.game": {
        "mc.network.protocol.game.join": [
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
        ],
        "mc.network.protocol.game.chunk": [
            "ClientboundLevelChunkWithLightPacket.java",
            "ClientboundLevelChunkPacketData.java",
            "ClientboundLightUpdatePacket.java",
            "ClientboundLightUpdatePacketData.java",
            "ClientboundChunkBatchStartPacket.java",
            "ClientboundChunkBatchFinishedPacket.java",
        ],
        "mc.network.protocol.game.serverbound": [
            "ServerboundMovePlayerPacket.java",
            "ServerboundChunkBatchReceivedPacket.java",
            "ServerboundAcceptTeleportationPacket.java",
            "ServerboundClientCommandPacket.java",
            "ServerboundClientTickEndPacket.java",
            "ServerboundPlayerActionPacket.java",
        ],
    },
    # ---- mc.world.level.* (issue #176, M2). Right-sized class clusters so each
    # M2 worldgen wave owns a file set that matches its narrative scope. The
    # levelgen root, biome, feature and feature.configurations are fully
    # partitioned (no residual row): every file lands in exactly one cluster, so
    # the pre-split package row disappears and external deps on the package
    # resolve to the lowest-id cluster (the wave-picker's shared-java_package
    # rule); all clusters stay in the giant SCC (cycle 27) and co-schedule, so
    # this is a scheduling non-issue. The other four packages (structure,
    # structures, templatesystem, chunk) keep a residual tail. Every concrete
    # feature and every structure+pieces pair is its own cluster: the 68-class
    # feature "SCC" and the structure fan-out each connect through a generated
    # registry hub and the framework base (see SPLIT_NOTES and the structure
    # SPLIT_EDGES), so once the hub tables are generated (and the framework is
    # ported), leaves -> core is a clean DAG.
    # net.minecraft.data.worldgen (issue #177/#178 prereq): the 3-file
    # bootstrap/terrain prerequisites split out of the 29-file monolithic
    # mc.data.worldgen row. The residual keeps the pre-split id (mc.data.worldgen)
    # and its 26-file complement (pools/pieces/structures/features registries),
    # so external dependents on net.minecraft.data.worldgen (the data rows in
    # wave 3's cycle) still resolve to one hub. mc.data.worldgen.prereq is
    # right-sized and not needs_split; the residual stays oversized and
    # needs_split=yes. Same-package refs from the residual into the prereq
    # (e.g. SurfaceRuleData/StructureSets referencing TerrainProvider or the
    # bootstraps) are deliberately NOT dep edges (recorded in SPLIT_NOTES): the
    # residual is not translated in this wave, so recording them would deadlock.
    # Crate override: the prereq's Java files import only net.minecraft.util +
    # net.minecraft.resources + net.minecraft.world.level.levelgen.synth, but the
    # ported modules live in rivet-world (next to Noises/NormalNoise/CubicSpline
    # usage); see CRATE_OVERRIDES.
    "net.minecraft.data.worldgen": {
        "mc.data.worldgen.prereq": [
            "BootstrapContext.java", "NoiseData.java", "TerrainProvider.java",
        ],
    },
    "net.minecraft.world.level.levelgen": {
        "mc.world.level.levelgen.random": [
            "BitRandomSource.java", "LegacyRandomSource.java",
            "MarsagliaPolarGaussian.java", "PositionalRandomFactory.java",
            "RandomSupport.java", "SingleThreadedRandomSource.java",
            "ThreadSafeLegacyRandomSource.java", "WorldgenRandom.java",
            "Xoroshiro128PlusPlus.java", "XoroshiroRandomSource.java",
        ],
        # The noise-router slice (#177 wave-1: the density-function + router value
        # layer) is separated from the noise-based chunk generator (noisegen: the
        # Aquifer/NoiseChunk/NoiseGeneratorSettings/OreVeinifier/RandomState/
        # NoiseRouterData class-level SCC plus NoiseBasedChunkGenerator, which
        # #183/#185 pick up with the generator stack). Verified against the Java:
        # noisegen -> noise is the only cross edge, and the noise -> random
        # (Noises -> PositionalRandomFactory) and noisegen -> settings
        # (NoiseBasedChunkGenerator -> BelowZeroRetrogen) edges are recorded in
        # SPLIT_EDGES.
        "mc.world.level.levelgen.noise": [
            "Beardifier.java", "Column.java", "Density.java",
            "DensityFunction.java", "DensityFunctions.java", "Heightmap.java",
            "NoiseRouter.java", "NoiseSettings.java", "Noises.java",
            "VerticalAnchor.java", "WorldGenerationContext.java", "package-info.java",
        ],
        "mc.world.level.levelgen.noisegen": [
            "Aquifer.java", "NoiseBasedChunkGenerator.java", "NoiseChunk.java",
            "NoiseGeneratorSettings.java", "NoiseRouterData.java",
            "OreVeinifier.java", "RandomState.java",
        ],
        "mc.world.level.levelgen.settings": [
            "BelowZeroRetrogen.java", "DebugLevelSource.java",
            "FlatLevelSource.java", "GenerationStep.java",
            "GeodeBlockSettings.java", "GeodeCrackSettings.java",
            "GeodeLayerSettings.java", "WorldDimensions.java",
            "WorldGenSettings.java", "WorldOptions.java",
        ],
        "mc.world.level.levelgen.surface": [
            "SurfaceRules.java", "SurfaceSystem.java",
        ],
        # PatrolSpawner/PhantomSpawner are CustomSpawner impls (mob spawning, not
        # surface rules); they ride with the world-tick spawner work, not with
        # the #179 surface-system unit.
        "mc.world.level.levelgen.spawner": [
            "PatrolSpawner.java", "PhantomSpawner.java",
        ],
    },
    "net.minecraft.world.level.levelgen.feature": {
        "mc.world.level.levelgen.feature.core": [
            "ConfiguredFeature.java", "Feature.java", "FeatureCountTracker.java",
            "FeaturePlaceContext.java", "package-info.java",
        ],
        "mc.world.level.levelgen.feature.selector": [
            "RandomSelectorFeature.java", "RandomBooleanSelectorFeature.java",
            "SimpleRandomSelectorFeature.java",
            "WeightedRandomSelectorFeature.java", "WeightedPlacedFeature.java",
            "SequenceFeature.java",
        ],
        "mc.world.level.levelgen.feature.tree": [
            "TreeFeature.java", "FallenTreeFeature.java",
        ],
        "mc.world.level.levelgen.feature.mushroom": [
            "AbstractHugeMushroomFeature.java", "HugeBrownMushroomFeature.java",
            "HugeRedMushroomFeature.java",
        ],
        "mc.world.level.levelgen.feature.ore": [
            "OreFeature.java", "ScatteredOreFeature.java",
        ],
        "mc.world.level.levelgen.feature.coral": [
            "CoralFeature.java", "CoralClawFeature.java",
            "CoralMushroomFeature.java", "CoralTreeFeature.java",
        ],
        "mc.world.level.levelgen.feature.speleothem": [
            "SpeleothemUtils.java", "SpeleothemFeature.java",
            "SpeleothemClusterFeature.java", "LargeDripstoneFeature.java",
        ],
        "mc.world.level.levelgen.feature.vegetation": [
            "VegetationPatchFeature.java", "WaterloggedVegetationPatchFeature.java",
            "MultifaceGrowthFeature.java", "RootSystemFeature.java",
            "NetherForestVegetationFeature.java",
        ],
        "mc.world.level.levelgen.feature.fungus": [
            "HugeFungusFeature.java", "HugeFungusConfiguration.java",
            "WeepingVinesFeature.java", "TwistingVinesFeature.java",
        ],
        # The 39 independent leaf units (#181). Each leaf is one file except the
        # fossil pair (FossilFeature + FossilFeatureConfiguration bundled, 2
        # files) and is genuinely independent: it references only feature.core
        # (Feature, FeaturePlaceContext, ConfiguredFeature) — verified against
        # the Java that no leaf names a sibling leaf except the fossil pair
        # below and the family back-refs in SPLIT_EDGES. So once feature.core
        # is done, every leaf is pickable in parallel (the hub's reverse
        # registration edges are generated content, not dep edges).
        "mc.world.level.levelgen.feature.bamboo": [
            "BambooFeature.java",
        ],
        "mc.world.level.levelgen.feature.basaltcolumns": [
            "BasaltColumnsFeature.java",
        ],
        "mc.world.level.levelgen.feature.basaltpillar": [
            "BasaltPillarFeature.java",
        ],
        "mc.world.level.levelgen.feature.blockblob": [
            "BlockBlobFeature.java",
        ],
        "mc.world.level.levelgen.feature.blockcolumn": [
            "BlockColumnFeature.java",
        ],
        "mc.world.level.levelgen.feature.blockpile": [
            "BlockPileFeature.java",
        ],
        "mc.world.level.levelgen.feature.blueice": [
            "BlueIceFeature.java",
        ],
        "mc.world.level.levelgen.feature.bonuschest": [
            "BonusChestFeature.java",
        ],
        "mc.world.level.levelgen.feature.chorusplant": [
            "ChorusPlantFeature.java",
        ],
        "mc.world.level.levelgen.feature.delta": [
            "DeltaFeature.java",
        ],
        "mc.world.level.levelgen.feature.desertwell": [
            "DesertWellFeature.java",
        ],
        "mc.world.level.levelgen.feature.disk": [
            "DiskFeature.java",
        ],
        "mc.world.level.levelgen.feature.endgateway": [
            "EndGatewayFeature.java",
        ],
        "mc.world.level.levelgen.feature.endisland": [
            "EndIslandFeature.java",
        ],
        "mc.world.level.levelgen.feature.endplatform": [
            "EndPlatformFeature.java",
        ],
        "mc.world.level.levelgen.feature.endpodium": [
            "EndPodiumFeature.java",
        ],
        "mc.world.level.levelgen.feature.endspike": [
            "EndSpikeFeature.java",
        ],
        "mc.world.level.levelgen.feature.filllayer": [
            "FillLayerFeature.java",
        ],
        "mc.world.level.levelgen.feature.fossil": [
            "FossilFeature.java", "FossilFeatureConfiguration.java",
        ],
        "mc.world.level.levelgen.feature.geode": [
            "GeodeFeature.java",
        ],
        "mc.world.level.levelgen.feature.glowstone": [
            "GlowstoneFeature.java",
        ],
        "mc.world.level.levelgen.feature.iceberg": [
            "IcebergFeature.java",
        ],
        "mc.world.level.levelgen.feature.kelp": [
            "KelpFeature.java",
        ],
        "mc.world.level.levelgen.feature.lake": [
            "LakeFeature.java",
        ],
        "mc.world.level.levelgen.feature.monsterroom": [
            "MonsterRoomFeature.java",
        ],
        "mc.world.level.levelgen.feature.noop": [
            "NoOpFeature.java",
        ],
        "mc.world.level.levelgen.feature.replaceblobs": [
            "ReplaceBlobsFeature.java",
        ],
        "mc.world.level.levelgen.feature.replaceblock": [
            "ReplaceBlockFeature.java",
        ],
        "mc.world.level.levelgen.feature.sculkpatch": [
            "SculkPatchFeature.java",
        ],
        "mc.world.level.levelgen.feature.seapickle": [
            "SeaPickleFeature.java",
        ],
        "mc.world.level.levelgen.feature.seagrass": [
            "SeagrassFeature.java",
        ],
        "mc.world.level.levelgen.feature.simpleblock": [
            "SimpleBlockFeature.java",
        ],
        "mc.world.level.levelgen.feature.snowandfreeze": [
            "SnowAndFreezeFeature.java",
        ],
        "mc.world.level.levelgen.feature.spike": [
            "SpikeFeature.java",
        ],
        "mc.world.level.levelgen.feature.spring": [
            "SpringFeature.java",
        ],
        "mc.world.level.levelgen.feature.template": [
            "TemplateFeature.java",
        ],
        "mc.world.level.levelgen.feature.underwatermagma": [
            "UnderwaterMagmaFeature.java",
        ],
        "mc.world.level.levelgen.feature.vines": [
            "VinesFeature.java",
        ],
        "mc.world.level.levelgen.feature.voidstartplatform": [
            "VoidStartPlatformFeature.java",
        ],
    },
    "net.minecraft.world.level.levelgen.feature.configurations": {
        "mc.world.level.levelgen.feature.configurations.core": [
            "FeatureConfiguration.java", "NoneFeatureConfiguration.java",
            "package-info.java",
        ],
        # The 37 independent leaf configurations (#181). Each leaf is one file
        # and references only configurations.core (FeatureConfiguration) —
        # verified no leaf names a sibling leaf except the two cross-refs in
        # SPLIT_EDGES (netherforestvegetation -> blockpile, replaceblock -> ore).
        "mc.world.level.levelgen.feature.configurations.blockblob": [
            "BlockBlobConfiguration.java",
        ],
        "mc.world.level.levelgen.feature.configurations.blockcolumn": [
            "BlockColumnConfiguration.java",
        ],
        "mc.world.level.levelgen.feature.configurations.blockpile": [
            "BlockPileConfiguration.java",
        ],
        "mc.world.level.levelgen.feature.configurations.blockstate": [
            "BlockStateConfiguration.java",
        ],
        "mc.world.level.levelgen.feature.configurations.columnfeature": [
            "ColumnFeatureConfiguration.java",
        ],
        "mc.world.level.levelgen.feature.configurations.compositefeature": [
            "CompositeFeatureConfiguration.java",
        ],
        "mc.world.level.levelgen.feature.configurations.count": [
            "CountConfiguration.java",
        ],
        "mc.world.level.levelgen.feature.configurations.deltafeature": [
            "DeltaFeatureConfiguration.java",
        ],
        "mc.world.level.levelgen.feature.configurations.disk": [
            "DiskConfiguration.java",
        ],
        "mc.world.level.levelgen.feature.configurations.endgateway": [
            "EndGatewayConfiguration.java",
        ],
        "mc.world.level.levelgen.feature.configurations.endspike": [
            "EndSpikeConfiguration.java",
        ],
        "mc.world.level.levelgen.feature.configurations.fallentree": [
            "FallenTreeConfiguration.java",
        ],
        "mc.world.level.levelgen.feature.configurations.geode": [
            "GeodeConfiguration.java",
        ],
        "mc.world.level.levelgen.feature.configurations.hugemushroomfeature": [
            "HugeMushroomFeatureConfiguration.java",
        ],
        "mc.world.level.levelgen.feature.configurations.largedripstone": [
            "LargeDripstoneConfiguration.java",
        ],
        "mc.world.level.levelgen.feature.configurations.layer": [
            "LayerConfiguration.java",
        ],
        "mc.world.level.levelgen.feature.configurations.multifacegrowth": [
            "MultifaceGrowthConfiguration.java",
        ],
        "mc.world.level.levelgen.feature.configurations.netherforestvegetation": [
            "NetherForestVegetationConfig.java",
        ],
        "mc.world.level.levelgen.feature.configurations.ore": [
            "OreConfiguration.java",
        ],
        "mc.world.level.levelgen.feature.configurations.probabilityfeature": [
            "ProbabilityFeatureConfiguration.java",
        ],
        "mc.world.level.levelgen.feature.configurations.randombooleanfeature": [
            "RandomBooleanFeatureConfiguration.java",
        ],
        "mc.world.level.levelgen.feature.configurations.randomfeature": [
            "RandomFeatureConfiguration.java",
        ],
        "mc.world.level.levelgen.feature.configurations.replaceblock": [
            "ReplaceBlockConfiguration.java",
        ],
        "mc.world.level.levelgen.feature.configurations.replacesphere": [
            "ReplaceSphereConfiguration.java",
        ],
        "mc.world.level.levelgen.feature.configurations.rootsystem": [
            "RootSystemConfiguration.java",
        ],
        "mc.world.level.levelgen.feature.configurations.sculkpatch": [
            "SculkPatchConfiguration.java",
        ],
        "mc.world.level.levelgen.feature.configurations.simpleblock": [
            "SimpleBlockConfiguration.java",
        ],
        "mc.world.level.levelgen.feature.configurations.speleothemcluster": [
            "SpeleothemClusterConfiguration.java",
        ],
        "mc.world.level.levelgen.feature.configurations.speleothem": [
            "SpeleothemConfiguration.java",
        ],
        "mc.world.level.levelgen.feature.configurations.spike": [
            "SpikeConfiguration.java",
        ],
        "mc.world.level.levelgen.feature.configurations.spring": [
            "SpringConfiguration.java",
        ],
        "mc.world.level.levelgen.feature.configurations.templatefeature": [
            "TemplateFeatureConfiguration.java",
        ],
        "mc.world.level.levelgen.feature.configurations.tree": [
            "TreeConfiguration.java",
        ],
        "mc.world.level.levelgen.feature.configurations.twistingvines": [
            "TwistingVinesConfig.java",
        ],
        "mc.world.level.levelgen.feature.configurations.underwatermagma": [
            "UnderwaterMagmaConfiguration.java",
        ],
        "mc.world.level.levelgen.feature.configurations.vegetationpatch": [
            "VegetationPatchConfiguration.java",
        ],
        "mc.world.level.levelgen.feature.configurations.weightedrandomfeature": [
            "WeightedRandomFeatureConfiguration.java",
        ],
    },
    "net.minecraft.world.level.biome": {
        "mc.world.level.biome.core": [
            "Biome.java", "BiomeGenerationSettings.java", "BiomeManager.java",
            "BiomeResolver.java", "BiomeSpecialEffects.java", "Climate.java",
            "MobSpawnSettings.java", "package-info.java",
        ],
        "mc.world.level.biome.source": [
            "BiomeSource.java", "BiomeSources.java",
            "CheckerboardColumnBiomeSource.java", "FixedBiomeSource.java",
            "MultiNoiseBiomeSource.java", "MultiNoiseBiomeSourceParameterList.java",
            "MultiNoiseBiomeSourceParameterLists.java", "TheEndBiomeSource.java",
        ],
        "mc.world.level.biome.data": [
            "Biomes.java", "FeatureSorter.java", "OverworldBiomeBuilder.java",
        ],
    },
    "net.minecraft.world.level.levelgen.structure": {
        # The framework base (13 files: the Structure/StructureStart/StructureType
        # + StructurePiece/StructurePieceAccessor SCCs, StructureSet, BoundingBox,
        # StructureCheck + StructureCheckResult, StructureSpawnOverride,
        # TerrainAdjustment, PostPlacementProcessor) plus the concrete piece base
        # classes (4 files: ScatteredFeaturePiece, SinglePieceStructure,
        # TemplateStructurePiece, PoolElementStructurePiece — each extends
        # StructurePiece and uses BoundingBox, verified). framework.pieces ->
        # framework is the only cross edge; the residual is the two generated
        # registry hubs. #182's seed-walk slice picks both framework clusters.
        "mc.world.level.levelgen.structure.framework": [
            "BoundingBox.java", "PostPlacementProcessor.java",
            "Structure.java", "StructureCheck.java", "StructureCheckResult.java",
            "StructurePiece.java", "StructurePieceAccessor.java",
            "StructureSet.java", "StructureSpawnOverride.java",
            "StructureStart.java", "StructureType.java",
            "TerrainAdjustment.java", "package-info.java",
        ],
        "mc.world.level.levelgen.structure.framework.pieces": [
            "PoolElementStructurePiece.java", "ScatteredFeaturePiece.java",
            "SinglePieceStructure.java", "TemplateStructurePiece.java",
        ],
    },
    "net.minecraft.world.level.levelgen.structure.structures": {
        # Each concrete structure is its own Structure + Pieces pair; the pairs
        # are pairwise independent (an SCC only where a pair self-cycles:
        # Mineshaft, OceanRuin) and each depends on structure.framework (the
        # base classes), which is the near-perfect fan-out for #182. The
        # residual holds the M3-deferred nether/end pairs + package-info.
        "mc.world.level.levelgen.structure.structures.stronghold": [
            "StrongholdPieces.java", "StrongholdStructure.java",
        ],
        "mc.world.level.levelgen.structure.structures.oceanmonument": [
            "OceanMonumentPieces.java", "OceanMonumentStructure.java",
        ],
        "mc.world.level.levelgen.structure.structures.mineshaft": [
            "MineshaftPieces.java", "MineshaftStructure.java",
        ],
        "mc.world.level.levelgen.structure.structures.woodlandmansion": [
            "WoodlandMansionPieces.java", "WoodlandMansionStructure.java",
        ],
        "mc.world.level.levelgen.structure.structures.ruinedportal": [
            "RuinedPortalPiece.java", "RuinedPortalStructure.java",
        ],
        "mc.world.level.levelgen.structure.structures.desertpyramid": [
            "DesertPyramidPiece.java", "DesertPyramidStructure.java",
        ],
        "mc.world.level.levelgen.structure.structures.oceanruin": [
            "OceanRuinPieces.java", "OceanRuinStructure.java",
        ],
        "mc.world.level.levelgen.structure.structures.jungletemple": [
            "JungleTemplePiece.java", "JungleTempleStructure.java",
        ],
        "mc.world.level.levelgen.structure.structures.shipwreck": [
            "ShipwreckPieces.java", "ShipwreckStructure.java",
        ],
        "mc.world.level.levelgen.structure.structures.swamphut": [
            "SwampHutPiece.java", "SwampHutStructure.java",
        ],
        "mc.world.level.levelgen.structure.structures.igloo": [
            "IglooPieces.java", "IglooStructure.java",
        ],
        "mc.world.level.levelgen.structure.structures.buriedtreasure": [
            "BuriedTreasurePieces.java", "BuriedTreasureStructure.java",
        ],
        "mc.world.level.levelgen.structure.structures.jigsaw": [
            "JigsawStructure.java",
        ],
    },
    "net.minecraft.world.level.levelgen.structure.templatesystem": {
        # Fully partitioned into the processor framework (core), the rule-test
        # families (rules: the RuleTest/RuleTestType 8-file registry SCC + the
        # PosRuleTest/PosRuleTestType 5-file SCC, both self-contained) and the
        # concrete processors (processors, including the RuleProcessor/
        # ProcessorRule pair and the StructureProcessorTypes registry hub).
        # Verified against the Java: rules is an independent leaf, processors ->
        # rules (ProcessorRule -> RuleTest/PosRuleTest) + core, and the previous
        # core/residual 2-unit SCC dissolves into this clean DAG.
        "mc.world.level.levelgen.structure.templatesystem.core": [
            "LiquidSettings.java", "package-info.java",
            "StructurePlaceSettings.java", "StructureProcessor.java",
            "StructureProcessorList.java", "StructureProcessorType.java",
            "StructureTemplate.java", "StructureTemplateManager.java",
        ],
        "mc.world.level.levelgen.structure.templatesystem.rules": [
            "AlwaysTrueTest.java", "AxisAlignedLinearPosTest.java",
            "BlockMatchTest.java", "BlockStateMatchTest.java",
            "LinearPosTest.java", "PosAlwaysTrueTest.java",
            "PosRuleTest.java", "PosRuleTestType.java",
            "RandomBlockMatchTest.java", "RandomBlockStateMatchTest.java",
            "RuleTest.java", "RuleTestType.java", "TagMatchTest.java",
        ],
        "mc.world.level.levelgen.structure.templatesystem.processors": [
            "BlackstoneReplaceProcessor.java", "BlockAgeProcessor.java",
            "BlockIgnoreProcessor.java", "BlockRotProcessor.java",
            "CappedProcessor.java", "GravityProcessor.java",
            "JigsawReplacementProcessor.java", "LavaSubmergedBlockProcessor.java",
            "NopProcessor.java", "ProcessorRule.java",
            "ProtectedBlockProcessor.java", "RuleProcessor.java",
            "StructureProcessorTypes.java",
        ],
    },
    "net.minecraft.world.level.chunk": {
        # Fully partitioned: wire (palette-container value layer, aligns with the
        # M1 #108 scaffold in crates/rivet-world/src/chunk/), access (#183
        # in-memory chunk data SCC + ChunkSource provider seam), support
        # (dependency-free leaf types access builds on) and generator (#185
        # pipeline-facing generator stack). Strategy and package-info ride with
        # wire (Strategy is value-layer — verified no access/generator/support
        # file references it). UpgradeData lives in access, not support: chunk-
        # NBT upgrade operates on LevelChunk/LevelChunkSection, so support stays
        # a zero-cross-edge leaf.
        "mc.world.level.chunk.wire": [
            "Configuration.java", "GlobalPalette.java", "HashMapPalette.java",
            "LinearPalette.java", "MissingPaletteEntryException.java",
            "Palette.java", "PaletteResize.java", "PalettedContainer.java",
            "PalettedContainerFactory.java", "PalettedContainerRO.java",
            "SingleValuePalette.java", "Strategy.java", "package-info.java",
        ],
        "mc.world.level.chunk.access": [
            "ChunkAccess.java", "LevelChunk.java", "ProtoChunk.java",
            "EmptyLevelChunk.java", "ImposterProtoChunk.java",
            "LevelChunkSection.java", "BulkSectionAccess.java", "ChunkSource.java",
            "UpgradeData.java",
        ],
        "mc.world.level.chunk.support": [
            "BlockColumn.java", "CarvingMask.java", "DataLayer.java",
            "LightChunk.java", "LightChunkGetter.java", "StructureAccess.java",
        ],
        "mc.world.level.chunk.generator": [
            "ChunkGenerator.java", "ChunkGenerators.java",
            "ChunkGeneratorStructureState.java",
        ],
    },
    "net.minecraft.world.level.storage": {
        # Residual style (NOT fully partitioned): LevelStorageSource is the
        # on-disk access hub (region/level.dat/playerdata). The residual keeps
        # the pre-split id so the 100+ external dependents resolve here, not to
        # the lowest-id cluster (leveldata, which owns only the LevelData
        # interface family). The four foundation clusters (version, leveldata,
        # saveddata, value) are independent leaves; primaryleveldata builds on
        # leveldata + version (PrimaryLevelData is the level.dat load result
        # carrier produced alongside LevelStorageSource).
        "mc.world.level.storage.version": [
            "DataVersion.java", "LevelVersion.java",
        ],
        "mc.world.level.storage.leveldata": [
            "LevelData.java", "WritableLevelData.java", "ServerLevelData.java",
            "WorldData.java",
        ],
        "mc.world.level.storage.primaryleveldata": [
            "PrimaryLevelData.java", "DerivedLevelData.java",
            "LevelDataAndDimensions.java",
        ],
        "mc.world.level.storage.saveddata": [
            "SavedDataStorage.java", "CommandStorage.java",
        ],
        "mc.world.level.storage.value": [
            "TagValueInput.java", "TagValueOutput.java", "ValueInput.java",
            "ValueInputContextHelper.java", "ValueOutput.java",
        ],
    },
    "net.minecraft.world.level.lighting": {
        # Fully partitioned into the value/interface layer (core: the light-array
        # storage + graph structures + the LightEventListener interfaces) and the
        # engines (engine: LightEngine + the section storages + the block/sky/
        # level engines). Verified against the Java: engine -> core is the only
        # cross edge (LightEngine -> DataLayerStorageMap; the section storages ->
        # DataLayerStorageMap); the LightEngine <-> LayerLightSectionStorage cycle
        # stays inside engine. This is the #184 (M2 lighting) deliverable.
        "mc.world.level.lighting.core": [
            "DataLayerStorageMap.java", "DynamicGraphMinFixedPoint.java",
            "LayerLightEventListener.java", "LeveledPriorityQueue.java",
            "LightEventListener.java", "SpatialLongSet.java", "package-info.java",
        ],
        "mc.world.level.lighting.engine": [
            "BlockLightEngine.java", "BlockLightSectionStorage.java",
            "ChunkSkyLightSources.java", "LayerLightSectionStorage.java",
            "LevelLightEngine.java", "LightEngine.java",
            "SkyLightEngine.java", "SkyLightSectionStorage.java",
        ],
    },
    "net.minecraft.world.level.levelgen.blockpredicates": {
        # Fully partitioned star on BlockPredicate/BlockPredicateType (the hub:
        # interface factories + the register() registry). combinators/states/
        # simple each reference only core (BlockPredicate, BlockPredicateType,
        # CombiningPredicate, StateTestingPredicate — verified against the Java);
        # the hub's reverse registration edges are generated content like
        # Feature.java. This is the #180 (worldgen wave 4) deliverable. The
        # previous claim that blockpredicates was an irreducible SCC was wrong —
        # the 14 concrete predicates are pairwise-independent leaves.
        "mc.world.level.levelgen.blockpredicates.core": [
            "BlockPredicate.java", "BlockPredicateType.java",
            "CombiningPredicate.java", "StateTestingPredicate.java",
            "package-info.java",
        ],
        "mc.world.level.levelgen.blockpredicates.combinators": [
            "AllOfPredicate.java", "AnyOfPredicate.java", "NotPredicate.java",
        ],
        "mc.world.level.levelgen.blockpredicates.states": [
            "HasSturdyFacePredicate.java", "MatchingBlocksPredicate.java",
            "MatchingBlockTagPredicate.java", "MatchingFluidsPredicate.java",
            "ReplaceablePredicate.java", "SolidPredicate.java",
        ],
        "mc.world.level.levelgen.blockpredicates.simple": [
            "InsideWorldBoundsPredicate.java", "MatchingBiomesPredicate.java",
            "TrueBlockPredicate.java", "UnobstructedPredicate.java",
            "WouldSurvivePredicate.java",
        ],
    },
    "net.minecraft.world.level.levelgen.placement": {
        # Fully partitioned star on PlacementModifier/PlacementContext/PlacedFeature
        # /PlacementFilter/PlacementModifierType (core). The concrete placement
        # modifiers each reference only core (verified); the PlacementModifierType
        # registry hub's reverse registration edges are generated content like
        # Feature.java, so the 21-file "SCC" dissolves into core + leaves. The
        # count/repeating family (RepeatingPlacement + the three CountPlacement
        # subclasses) and the filter family (BiomeFilter/BlockPredicateFilter/
        # RarityFilter/Surface*Filter) are grouped by inheritance; the remaining
        # simple modifiers ride together. CaveSurface is a standalone enum (only
        # referenced by SurfaceRules in the surface cluster). This is the #181
        # (worldgen wave 5) placement half.
        "mc.world.level.levelgen.placement.core": [
            "PlacedFeature.java", "PlacementContext.java", "PlacementFilter.java",
            "PlacementModifier.java", "PlacementModifierType.java",
            "package-info.java",
        ],
        "mc.world.level.levelgen.placement.repeating": [
            "CountPlacement.java", "NoiseBasedCountPlacement.java",
            "NoiseThresholdCountPlacement.java", "RepeatingPlacement.java",
        ],
        "mc.world.level.levelgen.placement.filter": [
            "BiomeFilter.java", "BlockPredicateFilter.java", "RarityFilter.java",
            "SurfaceRelativeThresholdFilter.java", "SurfaceWaterDepthFilter.java",
        ],
        "mc.world.level.levelgen.placement.simple": [
            "CaveSurface.java", "CountOnEveryLayerPlacement.java",
            "EnvironmentScanPlacement.java", "FixedPlacement.java",
            "HeightRangePlacement.java", "HeightmapPlacement.java",
            "InSquarePlacement.java",
            "RandomOffsetPlacement.java",
        ],
    },
    # ---- net.minecraft.server.level (issue #227, M2 prerequisite for #185):
    # the 37-file / 13,246-LOC monolithic mc.server.level row splits into
    # right-sized class clusters around the eight pipeline targets (#185's
    # minimal region-streaming spine) plus a ticket value layer and a
    # worldgen view. Fully authored except the residual, which keeps the
    # pre-split id because 200+ external rows depend on net.minecraft.server.
    # level and must resolve to a single hub (mirror of the storage split). The
    # whole package is ONE class-level SCC (verified against the Java:
    # ChunkMap<->ServerChunkCache<->ServerLevel<->ServerPlayer and the holder/
    # distance/task/lights back-refs), so the split right-sizes file ownership;
    # it does not claim the units are cycle-free — every unit stays wave=3 /
    # cycle=27.
    #   - chunkmap (the hub): ChunkMap, the only concrete GeneratingChunkMap +
    #     ChunkHolder.PlayerProvider; owns all eight named targets' seams.
    #   - holder: ChunkHolder + GenerationChunkHolder + GeneratingChunkMap +
    #     ChunkGenerationTask — the generation-task layer is its own file and
    #     rides here (ChunkGenerationTask <-> GenerationChunkHolder is the
    #     tightest cycle, and both are Moonrise-scheduler STUBs in #185).
    #   - distance: DistanceManager (the ticket-priority graph + the
    #     per-player spawn tracker), consumed by chunkmap/servercache/tracker.
    #   - task: ChunkTaskDispatcher + ChunkTaskPriorityQueue +
    #     ThrottlingChunkTaskDispatcher (the Moonrise scheduler slots; the
    #     dispatcher implements ChunkHolder.LevelChangeListener).
    #   - level: ChunkLevel + FullChunkStatus + ChunkResult — the value layer
    #     everyone reads (level constants, status ladder, future-result
    #     carriers); the smallest independent leaf.
    #   - tracker: ChunkTracker + LoadingChunkTracker + SimulationChunkTracker
    #     + SectionTracker (the section-level twin) — the ticket-level
    #     propagation graph over DynamicGraphMinFixedPoint.
    #   - view: ChunkTrackingView (the square view-distance containment value).
    #   - servercache: ServerChunkCache (the ChunkSource facade + the
    #     MainThreadExecutor), the #185 spawn/tick entry.
    #   - light: ThreadedLevelLightEngine (the Starlight hook), feeding #184.
    #   - ticket: Ticket + TicketType (the ticket value layer, stored into
    #     the world-level TicketStorage seam by servercache/distance).
    #   - region: WorldGenRegion (the worldgen chunk-view container).
    #   - residual (mc.server.level): ServerLevel + ServerPlayer + the entity
    #     surface (ServerEntity/ServerEntityGetter/ServerPlayerGameMode) + the
    #     player/session value types (ServerBossEvent/DemoMode/PlayerSpawnFinder/
    #     PlayerMap/ChunkLoadCounter/BlockDestructionProgress/ColumnPos/
    #     ClientInformation/ParticleStatus) + package-info. The residual is the
    #     untranslated tail: every authored cluster references it (ServerLevel/
    #     ServerPlayer are the types the pipeline classes touch), and those
    #     back-refs are the #185 STUBs (see SPLIT_NOTES) — never dep edges.
    "net.minecraft.server.level": {
        "mc.server.level.pipeline.chunkmap": ["ChunkMap.java"],
        "mc.server.level.pipeline.holder": [
            "ChunkHolder.java", "GenerationChunkHolder.java",
            "GeneratingChunkMap.java", "ChunkGenerationTask.java",
        ],
        "mc.server.level.pipeline.distance": ["DistanceManager.java"],
        "mc.server.level.pipeline.task": [
            "ChunkTaskDispatcher.java", "ChunkTaskPriorityQueue.java",
            "ThrottlingChunkTaskDispatcher.java",
        ],
        "mc.server.level.pipeline.level": [
            "ChunkLevel.java", "FullChunkStatus.java", "ChunkResult.java",
        ],
        "mc.server.level.pipeline.tracker": [
            "ChunkTracker.java", "LoadingChunkTracker.java",
            "SimulationChunkTracker.java", "SectionTracker.java",
        ],
        "mc.server.level.pipeline.view": ["ChunkTrackingView.java"],
        "mc.server.level.pipeline.servercache": ["ServerChunkCache.java"],
        "mc.server.level.pipeline.light": ["ThreadedLevelLightEngine.java"],
        "mc.server.level.pipeline.ticket": ["Ticket.java", "TicketType.java"],
        "mc.server.level.pipeline.region": ["WorldGenRegion.java"],
    },
}

# Packages selected by the --split-network / --split-game / --split-world /
# --split-server flags.
WORLD_SPLIT_PACKAGES = {
    "net.minecraft.data.worldgen",
    "net.minecraft.world.level.levelgen",
    "net.minecraft.world.level.levelgen.feature",
    "net.minecraft.world.level.levelgen.feature.configurations",
    "net.minecraft.world.level.levelgen.blockpredicates",
    "net.minecraft.world.level.levelgen.placement",
    "net.minecraft.world.level.biome",
    "net.minecraft.world.level.levelgen.structure",
    "net.minecraft.world.level.levelgen.structure.structures",
    "net.minecraft.world.level.levelgen.structure.templatesystem",
    "net.minecraft.world.level.chunk",
    "net.minecraft.world.level.storage",
    "net.minecraft.world.level.lighting",
}
# Fully-partitioned packages emit no residual row: every on-disk file must land
# in an authored cluster, and the pre-split row id disappears (external deps on
# the package resolve to the lowest-id cluster via the wave-picker's
# shared-java_package rule). package_split_rows() fails fast if a file is left
# unowned or the pre-split row carries durable state that would be silently
# dropped.
FULLY_PARTITIONED = {
    "net.minecraft.world.level.levelgen",
    "net.minecraft.world.level.levelgen.feature",
    "net.minecraft.world.level.levelgen.feature.configurations",
    "net.minecraft.world.level.levelgen.blockpredicates",
    "net.minecraft.world.level.levelgen.placement",
    "net.minecraft.world.level.biome",
    "net.minecraft.world.level.levelgen.structure.templatesystem",
    "net.minecraft.world.level.chunk",
    "net.minecraft.world.level.lighting",
}
# Authored notes superseded by the current split shape (e.g. the feature
# residual note that described the now-explicit leaf clusters). They are not
# durable workflow state, so the fully-partitioned carry guard ignores them.
RETIRED_NOTES = {
    "residual: the remaining ~40 independent feature leaves are the #181 "
    "tail; they depend on feature.core (the hub's reverse registration edges "
    "are generated content, not dep edges)",
}
FLAG_PACKAGES: dict[str, set[str]] = {
    "network": {"net.minecraft.network"},
    "game": {"net.minecraft.network.protocol.game"},
    "world": WORLD_SPLIT_PACKAGES,
    "server": {"net.minecraft.server.level"},
}

# Authored same-package sibling deps (unit ids) that the wave-picker resolves
# exactly. These are the edges the package-level graph cannot see (Java
# same-package refs need no import): the residual -> its sub-units, plus the
# levelgen/biome/feature intra-cluster DAGs. Sub-unit -> residual edges are
# deliberately NOT recorded (see the same-package note in unit_row below): the
# residual is not translated in the same wave as its sub-units, so recording
# them would deadlock the wave — the translate-wave absorbs residual classes as
# STUBs instead. Every authored edge must target a real manifest row id: a
# dropped edge (or a stale one pointing at a now-fully-partitioned residual)
# would silently change wave-sequencing, so test_analyze_graph.py pins the full
# edge set against the emitted manifest.
SPLIT_EDGES: dict[str, set[str]] = {
    "mc.network.framing": {"mc.network.buf"},
    "mc.network": {"mc.network.buf", "mc.network.framing"},
    "mc.network.protocol.game": {
        "mc.network.protocol.game.join",
        "mc.network.protocol.game.chunk",
        "mc.network.protocol.game.serverbound",
    },
    # data.worldgen: the residual mc.data.worldgen -> its prereq sub-unit.
    # Sub-unit back-refs into the residual (e.g. TerrainProvider has none; the
    # residual's SurfaceRuleData/StructureSets/etc. reference the prereq) are
    # deliberate STUBs (see the mc.data.worldgen.prereq note), so the edge is
    # only residual -> prereq.
    "mc.data.worldgen": {"mc.data.worldgen.prereq"},
    # levelgen root: the RNG leaf layer is the base; the density/noise-router
    # layer (noise) rides on random, the noise-based chunk generator (noisegen)
    # rides on noise + random, and settings (flat/debug/retrogen sources) and
    # surface (surface rule system) ride on noise + noisegen. The reverse
    # noisegen -> surface (NoiseGeneratorSettings -> SurfaceRules, RandomState ->
    # SurfaceSystem) and noisegen -> settings (NoiseBasedChunkGenerator ->
    # BelowZeroRetrogen) are deliberate STUBs (see the noisegen note), so the
    # intra-package DAG stays acyclic.
    "mc.world.level.levelgen.noise": {"mc.world.level.levelgen.random"},
    "mc.world.level.levelgen.noisegen": {
        "mc.world.level.levelgen.random", "mc.world.level.levelgen.noise",
    },
    "mc.world.level.levelgen.settings": {
        "mc.world.level.levelgen.noise", "mc.world.level.levelgen.noisegen",
    },
    "mc.world.level.levelgen.surface": {
        "mc.world.level.levelgen.noise", "mc.world.level.levelgen.noisegen",
    },
    # biome: core is the base; data (OverworldBiomeBuilder, Biomes,
    # FeatureSorter) builds on core, and source builds on both core and data —
    # the parameter lists live in data (MultiNoiseBiomeSourceParameterList.
    # generateOverworldBiomes -> new OverworldBiomeBuilder().addBiomes, and
    # MultiNoiseBiomeSource / TheEndBiomeSource reference OverworldBiomeBuilder
    # /Biomes), verified against the Java. data never references source.
    "mc.world.level.biome.source": {
        "mc.world.level.biome.core", "mc.world.level.biome.data",
    },
    "mc.world.level.biome.data": {"mc.world.level.biome.core"},
    # feature: a star on feature.core — every concrete feature extends Feature
    # and uses FeaturePlaceContext/ConfiguredFeature (same-package refs, so they
    # are authored edges). The core hub's reverse registration edges are
    # generated content, so no cycle. The 39 leaf units each reference only
    # feature.core (verified against the Java); the family clusters and leaves
    # together form the clean DAG once the hub is codegen'd.
    "mc.world.level.levelgen.feature.selector": {"mc.world.level.levelgen.feature.core"},
    "mc.world.level.levelgen.feature.tree": {"mc.world.level.levelgen.feature.core"},
    "mc.world.level.levelgen.feature.mushroom": {"mc.world.level.levelgen.feature.core"},
    "mc.world.level.levelgen.feature.ore": {"mc.world.level.levelgen.feature.core"},
    "mc.world.level.levelgen.feature.coral": {"mc.world.level.levelgen.feature.core"},
    "mc.world.level.levelgen.feature.speleothem": {"mc.world.level.levelgen.feature.core"},
    "mc.world.level.levelgen.feature.vegetation": {"mc.world.level.levelgen.feature.core"},
    "mc.world.level.levelgen.feature.fungus": {"mc.world.level.levelgen.feature.core"},
    "mc.world.level.levelgen.feature.bamboo": {"mc.world.level.levelgen.feature.core"},
    "mc.world.level.levelgen.feature.basaltcolumns": {"mc.world.level.levelgen.feature.core"},
    "mc.world.level.levelgen.feature.basaltpillar": {"mc.world.level.levelgen.feature.core"},
    "mc.world.level.levelgen.feature.blockblob": {"mc.world.level.levelgen.feature.core"},
    "mc.world.level.levelgen.feature.blockcolumn": {"mc.world.level.levelgen.feature.core"},
    "mc.world.level.levelgen.feature.blockpile": {"mc.world.level.levelgen.feature.core"},
    "mc.world.level.levelgen.feature.blueice": {"mc.world.level.levelgen.feature.core"},
    "mc.world.level.levelgen.feature.bonuschest": {"mc.world.level.levelgen.feature.core"},
    "mc.world.level.levelgen.feature.chorusplant": {"mc.world.level.levelgen.feature.core"},
    "mc.world.level.levelgen.feature.delta": {"mc.world.level.levelgen.feature.core"},
    "mc.world.level.levelgen.feature.desertwell": {"mc.world.level.levelgen.feature.core"},
    "mc.world.level.levelgen.feature.disk": {"mc.world.level.levelgen.feature.core"},
    "mc.world.level.levelgen.feature.endgateway": {"mc.world.level.levelgen.feature.core"},
    "mc.world.level.levelgen.feature.endisland": {"mc.world.level.levelgen.feature.core"},
    "mc.world.level.levelgen.feature.endplatform": {"mc.world.level.levelgen.feature.core"},
    "mc.world.level.levelgen.feature.endpodium": {"mc.world.level.levelgen.feature.core"},
    "mc.world.level.levelgen.feature.endspike": {"mc.world.level.levelgen.feature.core"},
    "mc.world.level.levelgen.feature.filllayer": {"mc.world.level.levelgen.feature.core"},
    "mc.world.level.levelgen.feature.fossil": {"mc.world.level.levelgen.feature.core"},
    "mc.world.level.levelgen.feature.geode": {"mc.world.level.levelgen.feature.core"},
    "mc.world.level.levelgen.feature.glowstone": {"mc.world.level.levelgen.feature.core"},
    "mc.world.level.levelgen.feature.iceberg": {"mc.world.level.levelgen.feature.core"},
    "mc.world.level.levelgen.feature.kelp": {"mc.world.level.levelgen.feature.core"},
    "mc.world.level.levelgen.feature.lake": {"mc.world.level.levelgen.feature.core"},
    "mc.world.level.levelgen.feature.monsterroom": {"mc.world.level.levelgen.feature.core"},
    "mc.world.level.levelgen.feature.noop": {"mc.world.level.levelgen.feature.core"},
    "mc.world.level.levelgen.feature.replaceblobs": {"mc.world.level.levelgen.feature.core"},
    "mc.world.level.levelgen.feature.replaceblock": {"mc.world.level.levelgen.feature.core"},
    "mc.world.level.levelgen.feature.sculkpatch": {"mc.world.level.levelgen.feature.core"},
    "mc.world.level.levelgen.feature.seapickle": {"mc.world.level.levelgen.feature.core"},
    "mc.world.level.levelgen.feature.seagrass": {"mc.world.level.levelgen.feature.core"},
    "mc.world.level.levelgen.feature.simpleblock": {"mc.world.level.levelgen.feature.core"},
    "mc.world.level.levelgen.feature.snowandfreeze": {"mc.world.level.levelgen.feature.core"},
    "mc.world.level.levelgen.feature.spike": {"mc.world.level.levelgen.feature.core"},
    "mc.world.level.levelgen.feature.spring": {"mc.world.level.levelgen.feature.core"},
    "mc.world.level.levelgen.feature.template": {"mc.world.level.levelgen.feature.core"},
    "mc.world.level.levelgen.feature.underwatermagma": {"mc.world.level.levelgen.feature.core"},
    "mc.world.level.levelgen.feature.vines": {"mc.world.level.levelgen.feature.core"},
    "mc.world.level.levelgen.feature.voidstartplatform": {"mc.world.level.levelgen.feature.core"},
    # feature.configurations: the same star, now on configurations.core. The 37
    # leaf configs each extend/implement FeatureConfiguration (same-package); two
    # cross-leaf refs are authored explicitly (netherforestvegetation -> blockpile
    # and replaceblock -> ore, verified against the Java).
    "mc.world.level.levelgen.feature.configurations.blockblob": {"mc.world.level.levelgen.feature.configurations.core"},
    "mc.world.level.levelgen.feature.configurations.blockcolumn": {"mc.world.level.levelgen.feature.configurations.core"},
    "mc.world.level.levelgen.feature.configurations.blockpile": {"mc.world.level.levelgen.feature.configurations.core"},
    "mc.world.level.levelgen.feature.configurations.blockstate": {"mc.world.level.levelgen.feature.configurations.core"},
    "mc.world.level.levelgen.feature.configurations.columnfeature": {"mc.world.level.levelgen.feature.configurations.core"},
    "mc.world.level.levelgen.feature.configurations.compositefeature": {"mc.world.level.levelgen.feature.configurations.core"},
    "mc.world.level.levelgen.feature.configurations.count": {"mc.world.level.levelgen.feature.configurations.core"},
    "mc.world.level.levelgen.feature.configurations.deltafeature": {"mc.world.level.levelgen.feature.configurations.core"},
    "mc.world.level.levelgen.feature.configurations.disk": {"mc.world.level.levelgen.feature.configurations.core"},
    "mc.world.level.levelgen.feature.configurations.endgateway": {"mc.world.level.levelgen.feature.configurations.core"},
    "mc.world.level.levelgen.feature.configurations.endspike": {"mc.world.level.levelgen.feature.configurations.core"},
    "mc.world.level.levelgen.feature.configurations.fallentree": {"mc.world.level.levelgen.feature.configurations.core"},
    "mc.world.level.levelgen.feature.configurations.geode": {"mc.world.level.levelgen.feature.configurations.core"},
    "mc.world.level.levelgen.feature.configurations.hugemushroomfeature": {"mc.world.level.levelgen.feature.configurations.core"},
    "mc.world.level.levelgen.feature.configurations.largedripstone": {"mc.world.level.levelgen.feature.configurations.core"},
    "mc.world.level.levelgen.feature.configurations.layer": {"mc.world.level.levelgen.feature.configurations.core"},
    "mc.world.level.levelgen.feature.configurations.multifacegrowth": {"mc.world.level.levelgen.feature.configurations.core"},
    "mc.world.level.levelgen.feature.configurations.netherforestvegetation": {
        "mc.world.level.levelgen.feature.configurations.core",
        "mc.world.level.levelgen.feature.configurations.blockpile",
    },
    "mc.world.level.levelgen.feature.configurations.ore": {"mc.world.level.levelgen.feature.configurations.core"},
    "mc.world.level.levelgen.feature.configurations.probabilityfeature": {"mc.world.level.levelgen.feature.configurations.core"},
    "mc.world.level.levelgen.feature.configurations.randombooleanfeature": {"mc.world.level.levelgen.feature.configurations.core"},
    "mc.world.level.levelgen.feature.configurations.randomfeature": {"mc.world.level.levelgen.feature.configurations.core"},
    "mc.world.level.levelgen.feature.configurations.replaceblock": {
        "mc.world.level.levelgen.feature.configurations.core",
        "mc.world.level.levelgen.feature.configurations.ore",
    },
    "mc.world.level.levelgen.feature.configurations.replacesphere": {"mc.world.level.levelgen.feature.configurations.core"},
    "mc.world.level.levelgen.feature.configurations.rootsystem": {"mc.world.level.levelgen.feature.configurations.core"},
    "mc.world.level.levelgen.feature.configurations.sculkpatch": {"mc.world.level.levelgen.feature.configurations.core"},
    "mc.world.level.levelgen.feature.configurations.simpleblock": {"mc.world.level.levelgen.feature.configurations.core"},
    "mc.world.level.levelgen.feature.configurations.speleothemcluster": {"mc.world.level.levelgen.feature.configurations.core"},
    "mc.world.level.levelgen.feature.configurations.speleothem": {"mc.world.level.levelgen.feature.configurations.core"},
    "mc.world.level.levelgen.feature.configurations.spike": {"mc.world.level.levelgen.feature.configurations.core"},
    "mc.world.level.levelgen.feature.configurations.spring": {"mc.world.level.levelgen.feature.configurations.core"},
    "mc.world.level.levelgen.feature.configurations.templatefeature": {"mc.world.level.levelgen.feature.configurations.core"},
    "mc.world.level.levelgen.feature.configurations.tree": {"mc.world.level.levelgen.feature.configurations.core"},
    "mc.world.level.levelgen.feature.configurations.twistingvines": {"mc.world.level.levelgen.feature.configurations.core"},
    "mc.world.level.levelgen.feature.configurations.underwatermagma": {"mc.world.level.levelgen.feature.configurations.core"},
    "mc.world.level.levelgen.feature.configurations.vegetationpatch": {"mc.world.level.levelgen.feature.configurations.core"},
    "mc.world.level.levelgen.feature.configurations.weightedrandomfeature": {"mc.world.level.levelgen.feature.configurations.core"},
    # structure: every authored pair depends on the framework base AND the piece
    # base classes (each concrete Structure extends Structure /
    # SinglePieceStructure; each Pieces class extends StructurePiece/
    # ScatteredFeaturePiece/TemplateStructurePiece and uses BoundingBox —
    # verified against the Java). framework.pieces -> framework is the internal
    # edge; the pairs' file-level dep on net.minecraft.world.level.levelgen.
    # structure resolves to the residual, so the pair fan-out records the direct
    # framework + framework.pieces deps and stays a clean DAG. The reverse half
    # (StructureType in framework registers all 16 concrete structures) is hub
    # registration content like Feature.java (see the framework note). The
    # residual (generated BuiltinStructures/BuiltinStructureSets hubs) also
    # references the framework.
    "mc.world.level.levelgen.structure": {
        "mc.world.level.levelgen.structure.framework",
    },
    "mc.world.level.levelgen.structure.framework.pieces": {
        "mc.world.level.levelgen.structure.framework",
    },
    "mc.world.level.levelgen.structure.structures.stronghold": {
        "mc.world.level.levelgen.structure.framework",
        "mc.world.level.levelgen.structure.framework.pieces",
    },
    "mc.world.level.levelgen.structure.structures.oceanmonument": {
        "mc.world.level.levelgen.structure.framework",
        "mc.world.level.levelgen.structure.framework.pieces",
    },
    "mc.world.level.levelgen.structure.structures.mineshaft": {
        "mc.world.level.levelgen.structure.framework",
        "mc.world.level.levelgen.structure.framework.pieces",
    },
    "mc.world.level.levelgen.structure.structures.woodlandmansion": {
        "mc.world.level.levelgen.structure.framework",
        "mc.world.level.levelgen.structure.framework.pieces",
    },
    "mc.world.level.levelgen.structure.structures.ruinedportal": {
        "mc.world.level.levelgen.structure.framework",
        "mc.world.level.levelgen.structure.framework.pieces",
    },
    "mc.world.level.levelgen.structure.structures.desertpyramid": {
        "mc.world.level.levelgen.structure.framework",
        "mc.world.level.levelgen.structure.framework.pieces",
    },
    "mc.world.level.levelgen.structure.structures.oceanruin": {
        "mc.world.level.levelgen.structure.framework",
        "mc.world.level.levelgen.structure.framework.pieces",
    },
    "mc.world.level.levelgen.structure.structures.jungletemple": {
        "mc.world.level.levelgen.structure.framework",
        "mc.world.level.levelgen.structure.framework.pieces",
    },
    "mc.world.level.levelgen.structure.structures.shipwreck": {
        "mc.world.level.levelgen.structure.framework",
        "mc.world.level.levelgen.structure.framework.pieces",
    },
    "mc.world.level.levelgen.structure.structures.swamphut": {
        "mc.world.level.levelgen.structure.framework",
        "mc.world.level.levelgen.structure.framework.pieces",
    },
    "mc.world.level.levelgen.structure.structures.igloo": {
        "mc.world.level.levelgen.structure.framework",
        "mc.world.level.levelgen.structure.framework.pieces",
    },
    "mc.world.level.levelgen.structure.structures.buriedtreasure": {
        "mc.world.level.levelgen.structure.framework",
        "mc.world.level.levelgen.structure.framework.pieces",
    },
    "mc.world.level.levelgen.structure.structures.jigsaw": {
        "mc.world.level.levelgen.structure.framework",
        "mc.world.level.levelgen.structure.framework.pieces",
    },
    # templatesystem: fully partitioned into core (framework), rules (the
    # self-contained RuleTest/PosRuleTest families) and processors (the concrete
    # processors + the StructureProcessorTypes hub + the RuleProcessor/
    # ProcessorRule pair). processors builds on core + rules (RuleProcessor ->
    # RuleTest/PosRuleTest; every processor -> StructureProcessor/
    # StructurePlaceSettings/StructureTemplate); rules is an independent leaf.
    "mc.world.level.levelgen.structure.templatesystem.processors": {
        "mc.world.level.levelgen.structure.templatesystem.core",
        "mc.world.level.levelgen.structure.templatesystem.rules",
    },
    # lighting: engine (LightEngine + the section storages + the block/sky/level
    # engines) builds on core (the light-array storage + graph structures + the
    # LightEventListener interfaces). Verified against the Java: only engine ->
    # core edges; the LightEngine <-> LayerLightSectionStorage cycle is internal.
    "mc.world.level.lighting.engine": {"mc.world.level.lighting.core"},
    # blockpredicates: the three leaf groups reference only core (BlockPredicate,
    # BlockPredicateType, CombiningPredicate, StateTestingPredicate — verified).
    "mc.world.level.levelgen.blockpredicates.combinators": {
        "mc.world.level.levelgen.blockpredicates.core",
    },
    "mc.world.level.levelgen.blockpredicates.states": {
        "mc.world.level.levelgen.blockpredicates.core",
    },
    "mc.world.level.levelgen.blockpredicates.simple": {
        "mc.world.level.levelgen.blockpredicates.core",
    },
    # placement: the concrete modifier groups reference only core (PlacementModifier,
    # PlacementContext, PlacementFilter, PlacedFeature, PlacementModifierType —
    # verified); the placement.core hub's reverse registration edges are generated
    # content (see the core note).
    "mc.world.level.levelgen.placement.repeating": {
        "mc.world.level.levelgen.placement.core",
    },
    "mc.world.level.levelgen.placement.filter": {
        "mc.world.level.levelgen.placement.core",
    },
    "mc.world.level.levelgen.placement.simple": {
        "mc.world.level.levelgen.placement.core",
    },
    # chunk: access (the #183 in-memory chunk data SCC) builds on wire (the
    # palette container value layer) and support (the heightmap/light/carving
    # leaf types); generator (the #185 pipeline-facing stack) builds on access.
    # UpgradeData rides in access, so support is a zero-cross-edge leaf.
    # Verified against the Java: no wire -> non-wire edges at all.
    "mc.world.level.chunk.access": {
        "mc.world.level.chunk.wire", "mc.world.level.chunk.support",
    },
    "mc.world.level.chunk.generator": {"mc.world.level.chunk.access"},
    # storage: the residual (LevelStorageSource hub) builds on three of the
    # four foundation clusters (leveldata/value/version — saveddata is an
    # independent leaf the residual never references, verified) plus
    # primaryleveldata; primaryleveldata (PrimaryLevelData, the level.dat
    # load-result carrier) builds on leveldata + version. The foundation
    # clusters are independent leaves — no sub-unit -> residual back-edges
    # exist (verified), so no STUBs.
    "mc.world.level.storage": {
        "mc.world.level.storage.leveldata",
        "mc.world.level.storage.primaryleveldata",
        "mc.world.level.storage.value",
        "mc.world.level.storage.version",
    },
    "mc.world.level.storage.primaryleveldata": {
        "mc.world.level.storage.leveldata",
        "mc.world.level.storage.version",
    },
    # server.level (issue #227): the intra-cluster DAG, verified against the
    # Java. level is the value base every pipeline class reads (ChunkLevel
    # constants + FullChunkStatus + ChunkResult). holder builds on level
    # (GenerationChunkHolder.UNLOADED_CHUNK is a ChunkResult; ChunkHolder reads
    # FullChunkStatus). task builds on holder + level (ChunkTaskDispatcher
    # implements ChunkHolder.LevelChangeListener; ChunkTaskPriorityQueue reads
    # ChunkLevel.MAX_LEVEL). tracker builds on holder + level (LoadingChunkTracker
    # calls DistanceManager.getChunk/updateChunkScheduling -> ChunkHolder); the
    # tracker -> distance edge is deliberately NOT recorded (the real edge is
    # tracker consumes the abstract DistanceManager hooks, which is the #185
    # stub seam — see the tracker note). chunkmap builds on holder + level +
    # view (ChunkMap.getPlayers -> ChunkTrackingView; allChunksWithAtLeastStatus
    # -> ChunkLevel; the DistanceManager inner class is part of chunkmap).
    # distance builds on holder + level + tracker. light builds on chunkmap +
    # task. servercache builds on chunkmap + distance + holder + level + light +
    # ticket. region builds on holder (WorldGenRegion holds GenerationChunkHolder
    # references). The residual depends on chunkmap + holder + servercache +
    # ticket + view (ServerLevel -> chunkMap/getChunkSource, ServerPlayer ->
    # ChunkTrackingView/TicketType). Sub-unit back-refs into the residual
    # (ChunkMap.this.level is a ServerLevel, ServerChunkCache.level, DistanceManager
    # holds ServerPlayer, ThreadedLevelLightEngine casts to ServerLevel, ChunkHolder
    # -> ServerPlayer, WorldGenRegion -> ServerLevel) are
    # deliberate #185 STUBs, not dep edges — recording them would deadlock the wave.
    "mc.server.level": {
        "mc.server.level.pipeline.chunkmap",
        "mc.server.level.pipeline.holder",
        "mc.server.level.pipeline.servercache",
        "mc.server.level.pipeline.ticket",
        "mc.server.level.pipeline.view",
    },
    "mc.server.level.pipeline.holder": {"mc.server.level.pipeline.level"},
    "mc.server.level.pipeline.chunkmap": {
        "mc.server.level.pipeline.holder",
        "mc.server.level.pipeline.level",
        "mc.server.level.pipeline.view",
        "mc.server.level.pipeline.distance",
    },
    "mc.server.level.pipeline.distance": {
        "mc.server.level.pipeline.holder",
        "mc.server.level.pipeline.level",
        "mc.server.level.pipeline.tracker",
    },
    "mc.server.level.pipeline.task": {
        "mc.server.level.pipeline.holder",
        "mc.server.level.pipeline.level",
    },
    "mc.server.level.pipeline.tracker": {
        "mc.server.level.pipeline.holder",
        "mc.server.level.pipeline.level",
    },
    "mc.server.level.pipeline.servercache": {
        "mc.server.level.pipeline.holder",
        "mc.server.level.pipeline.level",
        "mc.server.level.pipeline.chunkmap",
        "mc.server.level.pipeline.distance",
        "mc.server.level.pipeline.light",
        "mc.server.level.pipeline.ticket",
    },
    "mc.server.level.pipeline.light": {
        "mc.server.level.pipeline.chunkmap",
        "mc.server.level.pipeline.task",
    },
    "mc.server.level.pipeline.region": {"mc.server.level.pipeline.holder"},
}

# Authored structural notes for split units (carried into the notes column,
# never clobbering a human triage note). These name the residual classes the
# translate-wave must absorb as STUBs because the same-package back-edge is not
# a dep edge, and the generated-content hubs whose registration tables come from
# rivet-codegen (extend the #49/#154 codegen) rather than being hand-ported.
SPLIT_NOTES: dict[str, str] = {
    "mc.network.protocol.game.join": (
        "M1 STUB: join packets implement Packet<ClientGamePacketListener> and "
        "register in GamePacketTypes (residual mc.network.protocol.game); "
        "translate-wave absorbs ClientGamePacketListener + GamePacketTypes as stubs"
    ),
    "mc.network.protocol.game.chunk": (
        "M1 STUB: chunk packets implement Packet<ClientGamePacketListener> and "
        "register in GamePacketTypes (residual mc.network.protocol.game); "
        "translate-wave absorbs ClientGamePacketListener + GamePacketTypes as stubs"
    ),
    "mc.network.protocol.game.serverbound": (
        "M1 STUB: serverbound packets implement Packet<ServerGamePacketListener> "
        "and register in GamePacketTypes (residual mc.network.protocol.game); "
        "translate-wave absorbs ServerGamePacketListener + GamePacketTypes as stubs"
    ),
    "mc.data.worldgen.prereq": (
        "#177/#178 prerequisite: the 3-file bootstrap/terrain slice "
        "(BootstrapContext/TerrainProvider/NoiseData) split out of the "
        "29-file mc.data.worldgen row into rivet-world::data::worldgen. "
        "BootstrapContext is the registry bootstrap contract (register + "
        "lookup); TerrainProvider is the overworld offset/factor/jaggedness "
        "CubicSpline builders + peaksAndValleys; NoiseData is the noise "
        "registration table (DEFAULT_SHIFT + the 63 declaration-ordered "
        "registrations). Crate override: net.minecraft.data normally maps to "
        "rivet-registry, but this unit's modules live in rivet-world next to "
        "the Noises/NormalNoise/CubicSpline layers they build on. STUBs: the "
        "real RegistrySetBuilder BootstrapContext implementation (duplicate-"
        "registration errors, UniversalLookup) defers to the registry-builder "
        "unit; the residual mc.data.worldgen classes that reference these "
        "(SurfaceRuleData/StructureSets/etc.) are the residual's STUBs, not "
        "dep edges"
    ),
    "mc.world.level.levelgen.noise": (
        "#177 wave-1: the density-function + noise-router value layer "
        "(Density/DensityFunction/DensityFunctions/NoiseRouter/NoiseRouterData's "
        "siblings/Noises/NoiseSettings). No STUBs: the reverse surface/settings "
        "back-refs live in noisegen (the NoiseGeneratorSettings -> SurfaceRules, "
        "RandomState -> SurfaceSystem and NoiseBasedChunkGenerator -> "
        "BelowZeroRetrogen edges are noisegen -> surface/settings STUBs) | "
        "Pulled ahead for #100: Heightmap + primeHeightmaps (the client heightmap "
        "long[] producer) in rivet-world::levelgen::heightmap; #287 Part A adds "
        "the worldgen/live slice (the FINAL/WORLDGEN predicates, the per-block "
        "update mutator, and the on-demand primeHeightmaps/getHeight compute on "
        "ChunkAccess); the #287 heightmap write serialization (write_heightmaps, "
        "the 4 FULL-status heightmaps in ordinal order) is ported in "
        "chunk::storage::serializable_chunk_data | "
        "#306 feature-shell wave: WorldGenerationContext.java (the minY/depth "
        "window) is proactively ported out-of-unit "
        "(levelgen::world_generation_context); the noise wave must not re-port it"
    ),
    "mc.world.level.levelgen.noisegen": (
        "#177 wave-1 + #183/#185: the noise-based chunk generator SCC (Aquifer, "
        "NoiseChunk, NoiseGeneratorSettings, NoiseRouterData, OreVeinifier, "
        "RandomState, NoiseBasedChunkGenerator) — the 6-file class-level SCC "
        "forces the #177 noise-router classes (NoiseGeneratorSettings, "
        "NoiseRouterData, RandomState) to ride with the generator classes. M2 "
        "STUB: NoiseGeneratorSettings -> SurfaceRules and RandomState -> "
        "SurfaceSystem (both in mc.world.level.levelgen.surface), and "
        "NoiseBasedChunkGenerator -> BelowZeroRetrogen (settings); translate-wave "
        "absorbs SurfaceRules/SurfaceSystem/BelowZeroRetrogen as stubs until the "
        "surface/settings units land"
    ),
    "mc.world.level.levelgen.carver": (
        "#306 feature-shell wave: the ConfiguredWorldCarver type shell is ported "
        "(levelgen::carver — the CarverConfiguration bound as a marker trait, "
        "WorldCarverId/WorldCarverBehavior, the ConfiguredWorldCarver record with "
        "isStartChunk dispatching through a STUB); the #180 algorithm (carve, "
        "CarvingContext, the concrete carvers + configurations, CarverDebugSettings) "
        "lands with this unit"
    ),
    "mc.world.level.levelgen.feature.core": (
        "#181 hub: generated content — Feature.java is the static registry hub "
        "(register(\"tree\", new TreeFeature(...)) x ~60); its registration table "
        "is emitted by rivet-codegen (per the #181 hub), so the 39 feature leaf "
        "units and 8 family clusters -> core is a clean DAG and the feature "
        "package's 68-class SCC dissolves"
    ),
    "mc.world.level.levelgen.feature.configurations.probabilityfeature": (
        "#306 feature-shell wave: ProbabilityFeatureConfiguration.java (the "
        "unit's single file) is proactively ported out-of-unit "
        "(levelgen::feature::configurations::probability_feature_configuration — "
        "the value class + floatRange record codec); the .probabilityfeature "
        "wave must not re-port it"
    ),
    "mc.world.level.levelgen.structure": (
        "#182 hub: BuiltinStructures/BuiltinStructureSets registration "
        "tables are emitted by rivet-codegen (extend #49/#154); the hubs "
        "reference structure.framework"
    ),
    "mc.world.level.levelgen.structure.structures": (
        "#182: M3-deferred tail — the EndCity/NetherFortress/NetherFossil "
        "structure + pieces pairs are not part of the M2 overworld seed-walk and "
        "stay in the residual; the 13 authored pairs each depend on "
        "structure.framework + structure.framework.pieces"
    ),
    "mc.world.level.levelgen.structure.templatesystem.core": (
        "#182: the processor framework (StructureProcessor/StructureProcessorList/"
        "StructureProcessorType/StructureTemplate/StructureTemplateManager/"
        "StructurePlaceSettings/LiquidSettings). Fully partitioned — the previous "
        "core/residual 2-unit SCC dissolves into core + rules + processors"
    ),
    "mc.world.level.levelgen.structure.templatesystem.rules": (
        "#182: the RuleTest/RuleTestType registry family (8-file SCC) and the "
        "PosRuleTest/PosRuleTestType family (5-file SCC), both self-contained; "
        "rules is an independent leaf the processors build on"
    ),
    "mc.world.level.levelgen.blockpredicates.core": (
        "#180: BlockPredicate is the interface hub (static factories returning "
        "each concrete predicate) and BlockPredicateType the register() registry; "
        "the hub's reverse registration edges are generated content like "
        "Feature.java (extend #49/#154), so combinators/states/simple -> core is "
        "a clean star"
    ),
    "mc.world.level.levelgen.placement.core": (
        "#181: PlacementModifierType is the register() registry hub over all "
        "concrete placement modifiers; its reverse registration edges are "
        "generated content like Feature.java (per the #181 hub), so the 21-file "
        "class-level 'SCC' dissolves into core + leaves. The core itself holds "
        "the PlacedFeature <-> PlacementContext and PlacementModifier <-> "
        "PlacementModifierType 2-node cycles (cycle 27, co-schedule)"
    ),
    "mc.world.level.lighting.engine": (
        "#184: LightEngine + the section storages (LayerLightSectionStorage/"
        "BlockLightSectionStorage/SkyLightSectionStorage) + the block/sky/level "
        "engines. The LightEngine <-> LayerLightSectionStorage 2-node class cycle "
        "is internal to this unit"
    ),
    "mc.world.level.lighting.core": (
        "#184: the light-array storage (DataLayerStorageMap/DynamicGraphMinFixedPoint/"
        "LeveledPriorityQueue/SpatialLongSet) + the LightEventListener/"
        "LayerLightEventListener interfaces the engines consume"
    ),
    "mc.world.level.chunk.wire": (
        "aligns with the M1 #108 scaffold in crates/rivet-world/src/chunk/ "
        "(palette.rs, paletted_container.rs, strategy.rs, configuration.rs)"
    ),
    "mc.world.level.chunk.access": (
        "#183: the in-memory chunk data SCC as generic value types in "
        "rivet-world::chunk (ChunkAccess base + LevelChunk/ProtoChunk/"
        "EmptyLevelChunk/ImposterProtoChunk) + the ChunkSource provider seam + "
        "the UpgradeData carrier; built on wire (palette container) + support "
        "(leaf types) + the world Heightmap.Types. The pending block-entity "
        "NBT map (get/set/remove_block_entity_nbt) and the stored heightmap/"
        "light reconstruction carry are ported (#337/#343); the live typed "
        "blockEntities map (setBlockEntity/getBlockEntity) and setBlockEntityNbt's "
        "containsKey guard (which reads that live map) defer with the "
        "block-entity unit. BulkSectionAccess is a chunk.access "
        "file deferred in place (owned by this unit, not ported this slice); "
        "the set-block/fluid accessors defer with the chunk-storage epic "
        "(#216), the ticks surface with the world.ticks unit, and the "
        "Starlight emptiness maps with the lighting units (#184)"
    ),
    "mc.world.level.chunk.support": (
        "#183: the dependency-free leaf types (BlockColumn/CarvingMask/DataLayer/"
        "LightChunk/LightChunkGetter/StructureAccess) that access builds on; "
        "zero cross-edge to access/generator"
    ),
    "mc.world.level": (
        "#306 feature-shell wave: WorldGenLevel.java (the worldgen write/read "
        "gate) is proactively ported out-of-unit (trait WorldGenLevel in "
        "rivet-world::level::world_gen_level — getSeed + ensureCanWrite, "
        "extending LevelHeightAccessor; setCurrentlyGenerating defers via "
        "RivetTodo #232); the mc.world.level wave must not re-port it"
    ),
    "mc.world.level.chunk.generator": (
        "#185: the generator stack (ChunkGenerator/ChunkGenerators/"
        "ChunkGeneratorStructureState) the pipeline-facing generator wave builds "
        "on | #306 feature-shell wave: the abstract ChunkGenerator seam is "
        "proactively ported out-of-unit (trait ChunkGenerator in "
        "rivet-world::chunk::chunk_generator — getMinY/getGenDepth pass-through; "
        "levelgen::feature/placement/WorldGenerationContext consume it as "
        "&dyn ChunkGenerator); the .chunk.generator wave must not re-port it"
    ),
    "mc.world.level.storage": (
        "#183: M2 hub — LevelStorageSource is the on-disk access hub "
        "(region/level.dat/playerdata); the residual keeps the pre-split id so "
        "external dependents resolve here. The storage split has no sub-unit "
        "back-edges, so no STUBs"
    ),
    "mc.world.level.biome.core": (
        "#178: the biome value core (Biome/BiomeGenerationSettings/BiomeManager/"
        "BiomeSpecialEffects/Climate/MobSpawnSettings); the base every other "
        "biome cluster builds on"
    ),
    "mc.world.level.biome.source": (
        "#178: the BiomeSource family (MultiNoiseBiomeSource/TheEndBiomeSource/"
        "CheckerboardColumnBiomeSource/FixedBiomeSource); builds on core + data"
    ),
    "mc.world.level.biome.data": (
        "#178: generated content — Biomes.java is a generated registry hub "
        "(extend #49/#154); Biomes + FeatureSorter + OverworldBiomeBuilder ride "
        "with #178"
    ),
    "mc.world.level.levelgen.random": (
        "#177 wave-1: the RNG leaf layer (WorldgenRandom/LegacyRandomSource/"
        "XoroshiroRandomSource/Xoroshiro128PlusPlus/PositionalRandomFactory); the "
        "base noise and noisegen build on"
    ),
    "mc.world.level.levelgen.settings": (
        "#179: the settings sources (FlatLevelSource/DebugLevelSource/"
        "WorldGenSettings/WorldDimensions/Geode*Settings); flat feeds M1.3 "
        "superflat (#100/#156). GenerationStep.java (the outer namespace "
        "holder for the Decoration enum) is proactively ported by the #306 "
        "feature-shell wave (levelgen::generation_step); the settings wave "
        "must not re-port it"
    ),
    "mc.world.level.levelgen.surface": (
        "#179 wave-3: the surface-rules system (SurfaceRules/SurfaceSystem); "
        "builds on noise + noisegen (the noisegen -> surface back-refs are the "
        "STUBs in the noisegen note)"
    ),
    "mc.world.level.levelgen.spawner": (
        "PatrolSpawner/PhantomSpawner are CustomSpawner impls (mob spawning, not "
        "surface rules); they ride with the world-tick spawner work, not #179"
    ),
    # ---- server.level (issue #227, prerequisite for #185). The pipeline
    # clusters are the #185 minimal region-streaming spine; every sub-unit's
    # back-references into the residual (ServerLevel/ServerPlayer) are #185
    # STUBs — the residual is not translated in the pipeline wave, so those
    # edges are deliberately absent from SPLIT_EDGES.
    "mc.server.level.pipeline.chunkmap": (
        "#185: ChunkMap is the chunk-pipeline hub (implements GeneratingChunkMap "
        "and ChunkHolder.PlayerProvider; owns the DistanceManager inner class, the "
        "TrackedEntity inner class -> ServerEntity, and the region storage read/"
        "write path). M2 STUB: ChunkMap.this.level / .playerMap are residual "
        "types (ServerLevel/ServerPlayer) — absorbed as stubs. The .lightEngine "
        "field (ThreadedLevelLightEngine) is a cross-cluster ref to the light "
        "cluster, deliberately not a dep edge (light -> chunkmap is the recorded "
        "edge; recording the reverse would cycle chunkmap<->light)"
    ),
    "mc.server.level.pipeline.holder": (
        "#185: ChunkHolder + GenerationChunkHolder + the generating-map seam "
        "(GeneratingChunkMap) + ChunkGenerationTask. The ChunkGenerationTask <-> "
        "GenerationChunkHolder 2-node class cycle is internal (Moonrise scheduler); "
        "#185 RivetTodos the 6k-LOC scheduler internals. M2 STUB: ChunkHolder/"
        "GenerationChunkHolder reference residual ServerPlayer/ServerLevel — "
        "absorbed as stubs. The ChunkMap refs (ChunkHolder.getChunkMap() -> "
        "(ChunkMap)this.playerProvider, GenerationChunkHolder.scheduleChunkGeneration"
        "Task(ChunkMap)) are cross-cluster reverse edges to the chunkmap cluster, "
        "deliberately not dep edges (chunkmap -> holder is the recorded edge). "
        "ChunkGenerationTask has no residual back-refs (only the "
        "GeneratingChunkMap/GenerationChunkHolder seams)"
    ),
    "mc.server.level.pipeline.distance": (
        "#185: DistanceManager is the ticket-priority graph (over TicketStorage) "
        "+ the per-player spawn tracker (PositionCountingAreaMap). M2 STUB: "
        "DistanceManager holds residual ServerPlayer — absorbed as stubs. The "
        "moonrise$getChunkMap() calls hit the chunkmap cluster (a cross-cluster "
        "reverse edge, deliberately not a dep: chunkmap -> distance is the "
        "recorded edge)"
    ),
    "mc.server.level.pipeline.task": (
        "#185: ChunkTaskDispatcher + ChunkTaskPriorityQueue + "
        "ThrottlingChunkTaskDispatcher — the Moonrise scheduler slots (rayon "
        "realization per D5); the dispatcher implements "
        "ChunkHolder.LevelChangeListener. Pure value layer: no residual "
        "back-refs (only the ChunkHolder.LevelChangeListener/ChunkLevel seams)"
    ),
    "mc.server.level.pipeline.level": (
        "#185: the value layer (ChunkLevel constants + the FullChunkStatus "
        "status ladder + the ChunkResult future-result carriers) every pipeline "
        "class reads; the smallest independent leaf"
    ),
    "mc.server.level.pipeline.tracker": (
        "#185: ChunkTracker/LoadingChunkTracker/SimulationChunkTracker + the "
        "section-level twin SectionTracker — the ticket-level propagation graph "
        "over DynamicGraphMinFixedPoint. The tracker -> distance edge is a "
        "deliberate #185 STUB (LoadingChunkTracker calls the abstract "
        "DistanceManager.getChunk/updateChunkScheduling hooks, which are the "
        "chunkmap seam)"
    ),
    "mc.server.level.pipeline.view": (
        "#185: ChunkTrackingView is the square view-distance containment value "
        "(contains/forEach/difference), consumed by ChunkMap and ServerPlayer; "
        "dependency-free leaf"
    ),
    "mc.server.level.pipeline.servercache": (
        "#185: ServerChunkCache is the ChunkSource facade + the MainThreadExecutor "
        "(BlockableEventLoop<Runnable>) + the spawn/tick entry; builds on the whole "
        "pipeline DAG. M2 STUB: ServerChunkCache.level is residual ServerLevel and "
        "the spawn iteration reads residual ServerPlayer (level.players()) — "
        "absorbed as stubs"
    ),
    "mc.server.level.pipeline.light": (
        "#185 + #184: ThreadedLevelLightEngine is the Starlight hook "
        "(StarLightLightingProvider); feeds the Starlight compute units in #184. "
        "M2 STUB: ThreadedLevelLightEngine casts to residual ServerLevel "
        "(the starlight$getLightEngine().getWorld() world) and reads residual "
        "ServerPlayer (the per-chunk moonrise$getPlayers list) — absorbed as "
        "stubs. The ChunkMap ref (final ChunkMap chunkMap field) is a "
        "cross-cluster dep on the chunkmap cluster, a real recorded edge "
        "(light -> chunkmap), not a stub"
    ),
    "mc.server.level.pipeline.ticket": (
        "#185: the ticket value layer (Ticket/TicketType); TicketType is the "
        "static registry of ticket types. Dependency-free leaf: the TicketStorage "
        "seam lives in net.minecraft.world.level (not this residual) and is "
        "consumed by servercache/distance, not by these value types"
    ),
    "mc.server.level.pipeline.region": (
        "#185: WorldGenRegion, the worldgen chunk-view container holding "
        "GenerationChunkHolder references (region -> holder). M2 STUB: "
        "WorldGenRegion.level is the residual ServerLevel seam — absorbed "
        "as stubs"
    ),
    "mc.server.level": (
        "#227 residual: the untranslated tail — ServerLevel/ServerPlayer + the "
        "entity surface (ServerEntity/ServerEntityGetter/ServerPlayerGameMode) + "
        "the player/session value types (ServerBossEvent/DemoMode/PlayerSpawnFinder/"
        "PlayerMap/ChunkLoadCounter/BlockDestructionProgress/ColumnPos/"
        "ClientInformation/ParticleStatus). Keeps the pre-split id so the 200+ "
        "external dependents on net.minecraft.server.level resolve to this hub; "
        "every pipeline sub-unit references it (the #185 STUB seam)"
    ),
}


# Unit-id crate overrides for split units whose ported modules live in a
# different crate than CRATE_RULES would assign. Every override must target a
# real split unit id; the test suite pins that each entry names an emitted unit
# and that a dropped override fails fast (a wrong crate silently misroutes the
# whole wave).
CRATE_OVERRIDES: dict[str, str] = {
    "mc.data.worldgen.prereq": "rivet-world",
}


def crate_for(pkg: str) -> str:
    if pkg == "net.minecraft":  # root-package classes only; subpackages match CRATE_RULES
        return "rivet-core"
    for prefix, crate in CRATE_RULES:
        if pkg == prefix or pkg.startswith(prefix + "."):
            return crate
    return "rivet-server"


def crate_for_unit(unit_id: str, pkg: str) -> str:
    return CRATE_OVERRIDES.get(unit_id, crate_for(pkg))


def derive_id(pkg: str) -> str:
    return (
        pkg.replace("net.minecraft.", "mc.")
        .replace("org.bukkit.", "bukkit.")
        .replace("io.papermc.paper.", "paper.")
    )


def tarjan_scc(nodes: list[str], edges: dict[str, set[str]]) -> list[list[str]]:
    index: dict[str, int] = {}
    lowlink: dict[str, int] = {}
    on_stack: set[str] = set()
    stack: list[str] = []
    sccs: list[list[str]] = []
    counter = 0

    for root in nodes:
        if root in index:
            continue
        # Iterative Tarjan: recursion overflows on ~700-node dense graphs.
        work: list[tuple[str, iter]] = [(root, iter(sorted(edges.get(root, ()))))]
        index[root] = lowlink[root] = counter
        counter += 1
        stack.append(root)
        on_stack.add(root)
        while work:
            node, it = work[-1]
            advanced = False
            for nxt in it:
                if nxt not in index:
                    index[nxt] = lowlink[nxt] = counter
                    counter += 1
                    stack.append(nxt)
                    on_stack.add(nxt)
                    work.append((nxt, iter(sorted(edges.get(nxt, ())))))
                    advanced = True
                    break
                if nxt in on_stack:
                    lowlink[node] = min(lowlink[node], index[nxt])
            if advanced:
                continue
            work.pop()
            if work:
                parent = work[-1][0]
                lowlink[parent] = min(lowlink[parent], lowlink[node])
            if lowlink[node] == index[node]:
                scc = []
                while True:
                    w = stack.pop()
                    on_stack.discard(w)
                    scc.append(w)
                    if w == node:
                        break
                sccs.append(scc)
    return sccs


def validate_ownership(rows: list[list[str]], roots: dict[str, Path]) -> None:
    """Fail fast unless every on-disk Java source is owned exactly once.

    Each java_paths entry is `root:relpath`. The multiset of those tokens over
    the whole manifest must equal the set of physical (root, relpath) files
    scanned: no duplicate ownership (a file declared in two units would be
    double-counted and silently dropped from a residual) and no file lost. This
    is the global invariant behind the per-split owned_by checks — it is what
    makes the four paper-api/paper-server package-info.java pairs (same relative
    path, two physical files) unambiguous.
    """
    owned: dict[str, str] = {}
    for r in rows:
        unit_id = r[0]
        for token in r[2].split(","):
            if token in owned:
                sys.exit(
                    f"duplicate physical ownership: {token} is declared in both "
                    f"{owned[token]} and {unit_id}; each file must be owned by "
                    f"exactly one unit"
                )
            owned[token] = unit_id
    expected = {
        f"{root}:{f.relative_to(roots[root]).as_posix()}"
        for root in roots
        for f in roots[root].rglob("*.java")
    }
    missing = expected - set(owned)
    if missing:
        sys.exit(
            "physical source not owned by any unit: "
            + ", ".join(sorted(missing)[:10])
            + ("" if len(missing) <= 10 else f" (+{len(missing) - 10} more)")
        )


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--split-nbt", action="store_true",
        help="split net.minecraft.nbt into class-cluster units (idempotent)",
    )
    parser.add_argument(
        "--split-network", action="store_true",
        help="split net.minecraft.network into class-cluster units (idempotent)",
    )
    parser.add_argument(
        "--split-game", action="store_true",
        help="split net.minecraft.network.protocol.game into join-critical "
        "class-cluster units (idempotent)",
    )
    parser.add_argument(
        "--split-world", action="store_true",
        help="split the oversized mc.world.level.* packages (issue #176) into "
        "right-sized class-cluster units (idempotent)",
    )
    parser.add_argument(
        "--split-server", action="store_true",
        help="split the oversized net.minecraft.server.level package (issue "
        "#227) into right-sized class-cluster units (idempotent)",
    )
    parser.add_argument(
        "--output", type=Path, default=None,
        help="write the manifest here instead of MANIFEST.tsv (tests)",
    )
    parser.add_argument(
        "--prev-manifest", type=Path, default=None,
        help="read durable status/attempts/notes from here instead of "
        "MANIFEST.tsv (tests)",
    )
    args = parser.parse_args()

    # A package can span several source roots (e.g. moonrise code appears under
    # both minecraft/java and main/java, and paper-api/paper-server share
    # io.papermc.paper packages), so each file carries its own root.
    pkg_files: dict[str, list[tuple[str, Path]]] = defaultdict(list)
    pkg_loc: dict[str, int] = defaultdict(int)
    pkg_deps: dict[str, set[str]] = defaultdict(set)
    file_deps: dict[Path, set[str]] = {}
    file_loc: dict[Path, int] = {}

    for root_name, root in ROOTS.items():
        if not root.is_dir():
            sys.exit(f"missing source root: {root} (run Paper's gradle setup first)")
        for f in root.rglob("*.java"):
            text = f.read_text(encoding="utf-8", errors="replace")
            m = PKG_RE.search(text)
            if not m:
                continue
            pkg = m.group(1)
            pkg_files[pkg].append((root_name, f))
            pkg_loc[pkg] += text.count("\n")
            file_loc[f] = text.count("\n")
            # Per-file internal deps feed the network class-cluster split: each
            # split unit's deps are the union over its own files' imports, so the
            # residual's deps are derived from source, never hand-enumerated.
            file_deps[f] = {
                imp.rsplit(".", 1)[0]
                for imp in IMP_RE.findall(text)
                if imp.startswith(INTERNAL_PREFIXES)
            }
            pkg_deps[pkg].update(file_deps[f])

    known = set(pkg_files)
    deps = {p: {d for d in pkg_deps[p] if d in known and d != p} for p in known}

    sccs = tarjan_scc(sorted(known), deps)
    scc_of: dict[str, int] = {}
    for i, scc in enumerate(sccs):
        for pkg in scc:
            scc_of[pkg] = i

    # Condensation + longest-path depth = wave number.
    scc_deps: dict[int, set[int]] = defaultdict(set)
    for pkg, ds in deps.items():
        for d in ds:
            if scc_of[d] != scc_of[pkg]:
                scc_deps[scc_of[pkg]].add(scc_of[d])

    wave: dict[int, int] = {}

    def depth(s: int) -> int:
        if s not in wave:
            wave[s] = 1 + max((depth(d) for d in scc_deps[s]), default=-1)
        return wave[s]

    sys.setrecursionlimit(100_000)
    for s in range(len(sccs)):
        depth(s)

    cycle_members = {i: scc for i, scc in enumerate(sccs) if len(scc) > 1}
    header = (
        "id\tjava_package\tjava_paths\tsource_root\tfiles\tloc\tcrate\twave\tcycle"
        "\tneeds_split\tdeps\tstatus\tattempts\tnotes"
    )

    # Durable workflow state from the previous manifest: id -> (status, attempts,
    # notes). Regeneration rewrites structure only; it never resets ported work.
    prev_state: dict[str, tuple[str, str, str]] = {}
    prev_manifest = args.prev_manifest or (REPO / "MANIFEST.tsv")
    if prev_manifest.exists():
        with prev_manifest.open(newline="", encoding="utf-8") as fh:
            for row in csv.DictReader(fh, delimiter="\t"):
                prev_state[row.get("id", "")] = (
                    row.get("status", "pending"),
                    row.get("attempts", "0"),
                    row.get("notes", ""),
                )

    def carry(unit_id: str, authored_notes: str) -> tuple[str, str, str]:
        # Regenerated content and human workflow state share the notes column:
        # keep the preserved note (human triage) and add the authored notes
        # (structural, e.g. cycle back-edges) when not already present, so
        # regeneration is idempotent and never clobbers a human note. Authored
        # notes may carry several " | "-separated segments; each segment lands
        # in the column independently so a unit with multiple structural notes
        # (e.g. a wave seam on top of its owning-note) survives a fresh
        # regeneration without being duplicated on the next carry.
        status, attempts, notes = prev_state.get(unit_id, ("pending", "0", ""))
        existing = notes.split(" | ") if notes else []
        for seg in (s for s in authored_notes.split(" | ") if s):
            if seg not in existing:
                notes = f"{notes} | {seg}".strip(" |")
                existing.append(seg)
        return status, attempts, notes

    def java_paths(pkg: str) -> str:
        # Root-qualified: `root:relpath`, sorted by (root, relpath), so a file
        # that physically exists under two roots is never ambiguous and the
        # ordering is deterministic even when two roots share a relative path.
        return ",".join(
            sorted(f"{root}:{f.relative_to(ROOTS[root]).as_posix()}"
                   for root, f in pkg_files[pkg])
        )

    def source_root(pkg: str) -> str:
        return ",".join(sorted({root for root, _ in pkg_files[pkg]}))

    def package_row(pkg: str) -> list[str]:
        unit_id = derive_id(pkg)
        in_cycle = scc_of[pkg] if scc_of[pkg] in cycle_members else ""
        # needs_split is actionable pre-translation split state: only oversized
        # units that are not yet done are candidates. Structural SCC pressure
        # stays visible in the cycle column, so a done unit never advertises
        # itself as needing a split (see the module docstring).
        status, attempts, notes = carry(unit_id, "")
        needs_split = "yes" if (
            status != "done" and len(pkg_files[pkg]) > SPLIT_FILE_THRESHOLD
        ) else ""
        return [
            unit_id, pkg, java_paths(pkg), source_root(pkg),
            str(len(pkg_files[pkg])), str(pkg_loc[pkg]), crate_for(pkg),
            str(wave[scc_of[pkg]]), str(in_cycle), needs_split,
            ",".join(sorted(deps[pkg])), status, attempts, notes,
        ]

    def split_row() -> list[str]:
        root_dir = ROOTS["minecraft"]
        nbt_dir = root_dir / NBT_DIR
        rows = []
        for unit_id, (pkg, files, wv, cyc, deps_list, authored_notes) in NBT_UNITS.items():
            loc = 0
            paths: list[str] = []
            for f in files:
                path = nbt_dir / f
                if not path.is_file():
                    sys.exit(f"--split-nbt: missing file for unit {unit_id}: {path}")
                loc += sum(1 for _ in path.open(encoding="utf-8", errors="replace"))
                paths.append(f"minecraft:{(NBT_DIR / f).as_posix()}")
            # deps may be java packages or unit ids (mc.nbt.snbt); both are kept.
            dep_str = ",".join(sorted(d for d in deps_list if d in known or d in NBT_UNITS))
            status, attempts, notes = carry(unit_id, authored_notes)
            # Same actionable gate as the base rows and the network/game splits:
            # only oversized units that are not yet done are splitting candidates
            # (all nbt units are done today, so the flag is empty in MANIFEST).
            needs_split = "yes" if (
                status != "done" and len(files) > SPLIT_FILE_THRESHOLD
            ) else ""
            rows.append([
                unit_id, pkg, ",".join(sorted(paths)), "minecraft",
                str(len(files)), str(loc), "rivet-nbt", str(wv), cyc, needs_split,
                dep_str, status, attempts, notes,
            ])
        return rows

    def package_split_rows(flag: str, pkgs: set[str]) -> list[list[str]]:
        """Class-cluster split shared by the network, game and world flags.

        For each split package: validate every authored file exists on disk and
        no file is declared twice, compute the residual as the complement of the
        authored lists within the package scan (so the split can never lose or
        duplicate a file), and emit one row per sub-unit plus the residual row
        (id == derive_id(pkg), so carry maps the pre-split row's durable state
        onto it). Sub-unit deps are the union of their own files' imports
        (mirroring the base scan's `d in known` filter, with the same-package
        self-edge dropped); same-package sibling edges are authored unit ids in
        SPLIT_EDGES (residual -> sub-units, plus the intra-cluster DAGs). A
        sub-unit's references back into the residual are deliberately NOT dep
        edges: the residual is not translated in the same wave as its sub-units,
        so recording them would deadlock the wave — the translate-wave absorbs
        those residual classes as STUBs instead (see SPLIT_NOTES). All units
        keep the package's wave/cycle (they remain inside the giant SCC). A
        package that is fully partitioned (FULLY_PARTITIONED: levelgen, biome,
        feature, feature.configurations) has no residual row: its pre-split row
        id disappears, and external deps on the package resolve to the lowest-id
        cluster (the wave-picker's shared-java_package rule). For those packages
        the fully-partitioned contract is enforced here: any file left outside
        the authored clusters is a hard error (it would silently materialize a
        residual row), and the pre-split row's durable state is a hard error too
        unless it is only a retired authored note (RETIRED_NOTES) — otherwise a
        future re-partition of a package with in-progress work would silently
        reset it.
        """
        root_dir = ROOTS["minecraft"]
        rows: list[list[str]] = []
        for pkg in sorted(pkgs):
            units = PACKAGE_SPLITS[pkg]
            pkg_dir = root_dir / pkg.replace(".", "/")
            all_files = sorted(p.name for p in pkg_dir.glob("*.java"))
            # Each file must be declared in exactly one unit: a cross-unit
            # duplicate would otherwise be double-counted (a set collapses it)
            # and silently drop out of the residual. Fail fast before any row is
            # emitted, naming both owning units.
            owned_by: dict[str, str] = {}
            for unit_id, files in units.items():
                for f in files:
                    if f not in all_files:
                        sys.exit(f"--split-{flag}: missing file for unit {unit_id}: "
                                 f"{pkg_dir / f}")
                    if f in owned_by:
                        sys.exit(f"--split-{flag}: {f} is declared in both "
                                 f"{owned_by[f]} and {unit_id}; each file must "
                                 f"belong to exactly one unit")
                    owned_by[f] = unit_id
            residual_files = [f for f in all_files if f not in owned_by]
            in_cycle = scc_of[pkg] if scc_of[pkg] in cycle_members else ""
            wave_n = wave[scc_of[pkg]]
            residual_id = derive_id(pkg)
            unit_ids = (*units.keys(), residual_id)
            authored_ids = set(units.keys())

            if pkg in FULLY_PARTITIONED:
                # Fully-partitioned contract: every file must be in an authored
                # cluster (a complement would mean a stale authored list silently
                # materializing a residual row) and the pre-split row id carries
                # no durable state that would be dropped when it disappears.
                if residual_files:
                    sys.exit(
                        f"--split-{flag}: {pkg} is fully partitioned but "
                        f"{len(residual_files)} file(s) are not declared in any "
                        f"cluster: {', '.join(sorted(residual_files)[:5])}"
                        + ("" if len(residual_files) <= 5
                           else f" (+{len(residual_files) - 5} more)")
                        + "; every file must land in an authored cluster"
                    )
                status, attempts, notes = prev_state.get(residual_id, ("pending", "0", ""))
                durable = status != "pending" or attempts != "0" or (
                    notes and notes not in RETIRED_NOTES
                )
                if durable:
                    sys.exit(
                        f"--split-{flag}: {residual_id} is fully partitioned and "
                        f"emits no residual row, so its pre-split durable state "
                        f"(status={status}, attempts={attempts}, notes={notes!r}) "
                        f"would be silently lost; carry it onto a cluster or "
                        f"retire it before re-splitting"
                    )

            def unit_row(unit_id: str, files: list[str]) -> list[str]:
                paths: list[str] = []
                loc = 0
                deps: set[str] = set()
                for f in files:
                    path = pkg_dir / f
                    paths.append(f"minecraft:{Path(pkg.replace('.', '/')) / f}")
                    loc += file_loc[path]
                    deps.update(file_deps[path])
                # Only real packages can be file-derived deps (mirror the base
                # scan's `d in known` filter); then drop the same-package
                # self-edge. Sibling units in the same package are named by unit
                # id so the wave-picker resolves them exactly (same mechanism as
                # the nbt split). A unit may still depend on a sibling package
                # (e.g. levelgen on net.minecraft.world.level.chunk), which is a
                # separate package row and stays a package dep.
                dep_tokens = {d for d in deps if d in known and d != pkg}
                dep_tokens.update(SPLIT_EDGES.get(unit_id, ()))
                status, attempts, notes = carry(unit_id, SPLIT_NOTES.get(unit_id, ""))
                needs_split = "yes" if (
                    status != "done" and len(files) > SPLIT_FILE_THRESHOLD
                ) else ""
                return [
                    unit_id, pkg, ",".join(sorted(paths)),
                    source_root(pkg), str(len(files)), str(loc), crate_for_unit(unit_id, pkg),
                    str(wave_n), str(in_cycle), needs_split,
                    ",".join(sorted(dep_tokens)), status, attempts, notes,
                ]

            for unit_id in unit_ids:
                if unit_id == residual_id and not residual_files:
                    # Fully-partitioned package (FULLY_PARTITIONED): no residual
                    # tail, so the pre-split row id disappears. External deps on
                    # the package resolve to the lowest-id cluster via the
                    # wave-picker's shared-java_package rule. (For a non-
                    # fully-partitioned package an empty complement is simply
                    # impossible here, since its residual is the complement of
                    # the authored lists within the package scan.)
                    continue
                files = units[unit_id] if unit_id in authored_ids else residual_files
                rows.append(unit_row(unit_id, files))
        return rows

    split_flags_on = {
        flag for flag, on in (("nbt", args.split_nbt),
                              ("network", args.split_network),
                              ("game", args.split_game),
                              ("world", args.split_world),
                              ("server", args.split_server))
        if on
    }
    rows = [
        r for pkg in sorted(known)
        if not (args.split_nbt and pkg in NBT_SPLIT_PACKAGES)
        if not any((
            args.split_network and pkg in FLAG_PACKAGES["network"],
            args.split_game and pkg in FLAG_PACKAGES["game"],
            args.split_world and pkg in FLAG_PACKAGES["world"],
            args.split_server and pkg in FLAG_PACKAGES["server"],
        ))
        for r in [package_row(pkg)]
    ]
    if args.split_nbt:
        rows.extend(split_row())
    for flag in ("network", "game", "world", "server"):
        if args.__dict__[f"split_{flag}"]:
            rows.extend(package_split_rows(flag, FLAG_PACKAGES[flag]))

    # Stable ordering: wave, then unit id.
    rows.sort(key=lambda r: (int(r[7]), r[0]))

    # Planned-id uniqueness (A3): a sub-unit id that collides with a package-
    # derived row or a sibling split unit id (e.g. a cluster named ".feature"
    # against the mc.world.level.levelgen.feature sub-package row) would be
    # silently clobbered by the wave-picker's by_id dict. Fail fast before
    # emission.
    ids = [r[0] for r in rows]
    dup_ids = sorted({i for i in ids if ids.count(i) > 1})
    if dup_ids:
        sys.exit(
            "duplicate unit ids in manifest: " + ", ".join(dup_ids)
            + "; every split unit id must be unique (a cluster cannot shadow a "
            "package-derived row or another cluster)"
        )

    validate_ownership(rows, ROOTS)

    out = args.output or (REPO / "MANIFEST.tsv")
    with out.open("w", encoding="utf-8") as fh:
        fh.write(header + "\n")
        for r in rows:
            fh.write("\t".join(r) + "\n")

    n_split = (len(NBT_UNITS) if args.split_nbt else 0)
    for flag in ("network", "game", "world", "server"):
        if not args.__dict__[f"split_{flag}"]:
            continue
        # Per split package: the sub-unit rows plus the residual row, minus the
        # skipped empty residual for a fully-partitioned package.
        for pkg in FLAG_PACKAGES[flag]:
            units = PACKAGE_SPLITS[pkg]
            owned = {f for files in units.values() for f in files}
            residual = [p.name for p in (ROOTS["minecraft"] / pkg.replace(".", "/"))
                        .glob("*.java") if p.name not in owned]
            n_split += len(units) + (1 if residual else 0)
    split_flags = " + ".join(sorted(split_flags_on))
    n_cycles = len(cycle_members)
    biggest = max(cycle_members.values(), key=len, default=[])
    print(f"{len(rows)} units -> {out}"
          + (f" ({split_flags} split: {n_split} class-cluster units)" if split_flags
             else ""))
    print(f"waves: 0..{max(wave.values())}, cycles: {n_cycles} "
          f"(largest {len(biggest)} pkgs), needs_split: "
          f"{sum(1 for r in rows if r[9] == 'yes')}")
    per_crate: dict[str, int] = defaultdict(int)
    for r in rows:
        per_crate[r[6]] += int(r[5])
    for crate, loc in sorted(per_crate.items(), key=lambda kv: -kv[1]):
        print(f"  {crate:22} {loc:>8} LOC")


if __name__ == "__main__":
    main()
