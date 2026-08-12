#!/usr/bin/env python3
"""Regression tests for scripts/analyze_graph.py's manifest generation.

Run with: python3 scripts/test_analyze_graph.py

Proves, against the real Paper tree under working/. analyze_graph.py hard-exits
if the Paper source roots are absent (it needs imports/LOC/cycle data from the
Java), so this suite requires the same working/ setup as the merge gate:

  1. `--split-network` and `--split-game` regeneration are byte-idempotent: two
     consecutive runs produce identical output (with carry applied between them).
  2. The baseline `--split-nbt --split-network --split-game --split-world`
     path still reproduces the committed MANIFEST.tsv byte-for-byte, so the nbt
     split, the network split, the game split, the world split and the package
     scan are untouched.
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
  3c. The 12 mc.world.level.* packages (issue #176) split into right-sized
      class-cluster units: levelgen, biome, feature, configurations,
      blockpredicates, placement, templatesystem, chunk and lighting are fully
      partitioned (no residual row), the other three (structure, structures,
      storage) keep a residual complement, the key clusters own exactly their
      authored files, inventory is conserved, every on-disk file is owned
      exactly once, wave/cycle is preserved (all stay wave=3/cycle=27), dep
      tokens resolve, carry holds, and a cross-unit duplicate fails fast.
  3d. The net.minecraft.server.level package (issue #227) splits into the
      11 pipeline clusters + a mc.server.level residual that keeps the pre-split
      id (the 200+ external dependents resolve to one hub); the residual is the
      complement, every on-disk file is owned exactly once, all 12 units stay
      wave=3/cycle=27, needs_split is cleared (all clusters and the residual are
      under the threshold), dep tokens resolve, durable state carries from the
      flat row onto the residual, and a cross-unit duplicate fails fast.
  4. wave/cycle metadata is preserved: all split units keep the package's
     wave and cycle (they remain inside the giant SCC).
  5. status/attempts/notes carry across regeneration, including on the split
     units (so the later protocol PR's status transitions survive a rerun).
  6. every dep token in the split manifest resolves to a unit via the
     wave-picker's rules (exact unit id, derived package id, or package match).
  7. Every java_paths entry is `root:relpath` (issue #173): the 4
     paper-api/paper-server package-info.java pairs are distinct, the rooted
     path multiset over the whole manifest has no duplicates, and it equals the
     on-disk (root, relpath) inventory. Duplicate physical ownership fails fast
     with an actionable diagnostic, and done units lose needs_split.
"""

import csv
import subprocess
import sys
import tempfile
from collections import Counter
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
ANALYZE = REPO / "scripts" / "analyze_graph.py"
MANIFEST = REPO / "MANIFEST.tsv"
sys.path.insert(0, str(REPO / "scripts"))
from wave_picker import resolve_dep  # noqa: E402

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

# ---- issue #176: mc.world.level.* class-cluster split -----------------------
LEVELGEN_PKG = "net.minecraft.world.level.levelgen"
FEATURE_PKG = "net.minecraft.world.level.levelgen.feature"
CONFIG_PKG = "net.minecraft.world.level.levelgen.feature.configurations"
BLOCKPREDICATES_PKG = "net.minecraft.world.level.levelgen.blockpredicates"
PLACEMENT_PKG = "net.minecraft.world.level.levelgen.placement"
BIOME_PKG = "net.minecraft.world.level.biome"
STRUCTURE_PKG = "net.minecraft.world.level.levelgen.structure"
STRUCTURES_PKG = "net.minecraft.world.level.levelgen.structure.structures"
TEMPLATESYSTEM_PKG = "net.minecraft.world.level.levelgen.structure.templatesystem"
CHUNK_PKG = "net.minecraft.world.level.chunk"
STORAGE_PKG = "net.minecraft.world.level.storage"
LIGHTING_PKG = "net.minecraft.world.level.lighting"
DATA_WORLDGEN_PKG = "net.minecraft.data.worldgen"
WORLD_PACKAGES = (LEVELGEN_PKG, FEATURE_PKG, CONFIG_PKG, BLOCKPREDICATES_PKG,
                  PLACEMENT_PKG, BIOME_PKG, STRUCTURE_PKG,
                  STRUCTURES_PKG, TEMPLATESYSTEM_PKG, CHUNK_PKG, STORAGE_PKG,
                  LIGHTING_PKG)
# data.worldgen (#177/#178 prereq): the 3-file bootstrap/terrain slice splits
# out of the 29-file monolithic row; the residual keeps the pre-split id.
DATA_WORLDGEN_PREREQ_FILES = {
    "BootstrapContext.java", "NoiseData.java", "TerrainProvider.java",
}
DATA_WORLDGEN_RESIDUAL_FILES = {
    "AncientCityStructurePieces.java", "AncientCityStructurePools.java",
    "BastionBridgePools.java", "BastionHoglinStablePools.java",
    "BastionHousingUnitsPools.java", "BastionPieces.java",
    "BastionSharedPools.java", "BastionTreasureRoomPools.java",
    "BiomeDefaultFeatures.java", "Carvers.java", "DesertVillagePools.java",
    "DimensionTypes.java", "PillagerOutpostPools.java", "PlainVillagePools.java",
    "Pools.java", "ProcessorLists.java", "SavannaVillagePools.java",
    "SnowyVillagePools.java", "StructureSets.java", "Structures.java",
    "SurfaceRuleData.java", "TaigaVillagePools.java",
    "TrailRuinsStructurePools.java", "TrialChambersStructurePools.java",
    "VillagePools.java", "package-info.java",
}
# Fully-partitioned packages emit no residual row: their pre-split row id
# disappears and external deps resolve to the lowest-id cluster.
FULLY_PARTITIONED = {LEVELGEN_PKG, FEATURE_PKG, CONFIG_PKG, BLOCKPREDICATES_PKG,
                     PLACEMENT_PKG, BIOME_PKG, TEMPLATESYSTEM_PKG, CHUNK_PKG,
                     LIGHTING_PKG}
# Key clusters pinned by the DoD: exact file ownership per cluster.
LEVELGEN_RANDOM_FILES = {
    "BitRandomSource.java", "LegacyRandomSource.java",
    "MarsagliaPolarGaussian.java", "PositionalRandomFactory.java",
    "RandomSupport.java", "SingleThreadedRandomSource.java",
    "ThreadSafeLegacyRandomSource.java", "WorldgenRandom.java",
    "Xoroshiro128PlusPlus.java", "XoroshiroRandomSource.java",
}
FEATURE_CORE_FILES = {
    "ConfiguredFeature.java", "Feature.java", "FeatureCountTracker.java",
    "FeaturePlaceContext.java", "package-info.java",
}
STRUCTURES_FILES = {
    "StrongholdPieces.java", "StrongholdStructure.java",
    "OceanMonumentPieces.java", "OceanMonumentStructure.java",
    "MineshaftPieces.java", "MineshaftStructure.java",
    "WoodlandMansionPieces.java", "WoodlandMansionStructure.java",
    "RuinedPortalPiece.java", "RuinedPortalStructure.java",
    "DesertPyramidPiece.java", "DesertPyramidStructure.java",
    "OceanRuinPieces.java", "OceanRuinStructure.java",
    "JungleTemplePiece.java", "JungleTempleStructure.java",
    "ShipwreckPieces.java", "ShipwreckStructure.java",
    "SwampHutPiece.java", "SwampHutStructure.java",
    "IglooPieces.java", "IglooStructure.java",
    "BuriedTreasurePieces.java", "BuriedTreasureStructure.java",
    "JigsawStructure.java",
}
CHUNK_WIRE_FILES = {
    "Configuration.java", "GlobalPalette.java", "HashMapPalette.java",
    "LinearPalette.java", "MissingPaletteEntryException.java", "Palette.java",
    "PaletteResize.java", "PalettedContainer.java",
    "PalettedContainerFactory.java", "PalettedContainerRO.java",
    "SingleValuePalette.java", "Strategy.java", "package-info.java",
}
CHUNK_ACCESS_FILES = {
    "ChunkAccess.java", "LevelChunk.java", "ProtoChunk.java",
    "EmptyLevelChunk.java", "ImposterProtoChunk.java", "LevelChunkSection.java",
    "BulkSectionAccess.java", "ChunkSource.java", "UpgradeData.java",
}
CHUNK_SUPPORT_FILES = {
    "BlockColumn.java", "CarvingMask.java", "DataLayer.java", "LightChunk.java",
    "LightChunkGetter.java", "StructureAccess.java",
}
CHUNK_GENERATOR_FILES = {
    "ChunkGenerator.java", "ChunkGenerators.java",
    "ChunkGeneratorStructureState.java",
}
# storage (residual style): the four foundation clusters + primaryleveldata are
# authored; the residual (derive_id(STORAGE_PKG) == mc.world.level.storage) owns
# LevelStorageSource + the on-disk access siblings + package-info.
STORAGE_VERSION_FILES = {"DataVersion.java", "LevelVersion.java"}
STORAGE_LEVELDATA_FILES = {
    "LevelData.java", "WritableLevelData.java", "ServerLevelData.java",
    "WorldData.java",
}
STORAGE_PRIMARY_FILES = {
    "PrimaryLevelData.java", "DerivedLevelData.java", "LevelDataAndDimensions.java",
}
STORAGE_SAVEDDATA_FILES = {"SavedDataStorage.java", "CommandStorage.java"}
STORAGE_VALUE_FILES = {
    "TagValueInput.java", "TagValueOutput.java", "ValueInput.java",
    "ValueInputContextHelper.java", "ValueOutput.java",
}
STORAGE_RESIDUAL_FILES = {
    "LevelStorageSource.java", "PlayerDataStorage.java", "LevelSummary.java",
    "LevelResource.java", "LevelStorageException.java",
    "FileNameDateFormatter.java", "package-info.java",
}
TEMPLATESYSTEM_CORE_FILES = {
    "LiquidSettings.java", "package-info.java", "StructurePlaceSettings.java",
    "StructureProcessor.java", "StructureProcessorList.java",
    "StructureProcessorType.java", "StructureTemplate.java",
    "StructureTemplateManager.java",
}
TEMPLATESYSTEM_RULES_FILES = {
    "AlwaysTrueTest.java", "AxisAlignedLinearPosTest.java", "BlockMatchTest.java",
    "BlockStateMatchTest.java", "LinearPosTest.java", "PosAlwaysTrueTest.java",
    "PosRuleTest.java", "PosRuleTestType.java", "RandomBlockMatchTest.java",
    "RandomBlockStateMatchTest.java", "RuleTest.java", "RuleTestType.java",
    "TagMatchTest.java",
}
TEMPLATESYSTEM_PROCESSORS_FILES = {
    "BlackstoneReplaceProcessor.java", "BlockAgeProcessor.java",
    "BlockIgnoreProcessor.java", "BlockRotProcessor.java", "CappedProcessor.java",
    "GravityProcessor.java", "JigsawReplacementProcessor.java",
    "LavaSubmergedBlockProcessor.java", "NopProcessor.java", "ProcessorRule.java",
    "ProtectedBlockProcessor.java", "RuleProcessor.java",
    "StructureProcessorTypes.java",
}
# levelgen noise/noisegen (#177/#183/#185): the density/noise-router value layer
# vs the noise-based chunk generator SCC.
LEVELGEN_NOISE_FILES = {
    "Beardifier.java", "Column.java", "Density.java", "DensityFunction.java",
    "DensityFunctions.java", "Heightmap.java", "NoiseRouter.java",
    "NoiseSettings.java", "Noises.java", "VerticalAnchor.java",
    "WorldGenerationContext.java", "package-info.java",
}
LEVELGEN_NOISEGEN_FILES = {
    "Aquifer.java", "NoiseBasedChunkGenerator.java", "NoiseChunk.java",
    "NoiseGeneratorSettings.java", "NoiseRouterData.java", "OreVeinifier.java",
    "RandomState.java",
}
# structure.framework / framework.pieces (#182): the base SCCs vs the concrete
# piece base classes.
STRUCTURE_FRAMEWORK_FILES = {
    "BoundingBox.java", "PostPlacementProcessor.java", "Structure.java",
    "StructureCheck.java", "StructureCheckResult.java", "StructurePiece.java",
    "StructurePieceAccessor.java", "StructureSet.java", "StructureSpawnOverride.java",
    "StructureStart.java", "StructureType.java", "TerrainAdjustment.java",
    "package-info.java",
}
STRUCTURE_FRAMEWORK_PIECES_FILES = {
    "PoolElementStructurePiece.java", "ScatteredFeaturePiece.java",
    "SinglePieceStructure.java", "TemplateStructurePiece.java",
}
# blockpredicates (#180): core + combinators/states/simple.
BLOCKPREDICATES_CORE_FILES = {
    "BlockPredicate.java", "BlockPredicateType.java", "CombiningPredicate.java",
    "StateTestingPredicate.java", "package-info.java",
}
BLOCKPREDICATES_COMBINATORS_FILES = {
    "AllOfPredicate.java", "AnyOfPredicate.java", "NotPredicate.java",
}
BLOCKPREDICATES_STATES_FILES = {
    "HasSturdyFacePredicate.java", "MatchingBlocksPredicate.java",
    "MatchingBlockTagPredicate.java", "MatchingFluidsPredicate.java",
    "ReplaceablePredicate.java", "SolidPredicate.java",
}
BLOCKPREDICATES_SIMPLE_FILES = {
    "InsideWorldBoundsPredicate.java", "MatchingBiomesPredicate.java",
    "TrueBlockPredicate.java", "UnobstructedPredicate.java",
    "WouldSurvivePredicate.java",
}
# placement (#181): core + repeating/filter/simple.
PLACEMENT_CORE_FILES = {
    "PlacedFeature.java", "PlacementContext.java", "PlacementFilter.java",
    "PlacementModifier.java", "PlacementModifierType.java", "package-info.java",
}
PLACEMENT_REPEATING_FILES = {
    "CountPlacement.java", "NoiseBasedCountPlacement.java",
    "NoiseThresholdCountPlacement.java", "RepeatingPlacement.java",
}
PLACEMENT_FILTER_FILES = {
    "BiomeFilter.java", "BlockPredicateFilter.java", "RarityFilter.java",
    "SurfaceRelativeThresholdFilter.java", "SurfaceWaterDepthFilter.java",
}
PLACEMENT_SIMPLE_FILES = {
    "CaveSurface.java", "CountOnEveryLayerPlacement.java",
    "EnvironmentScanPlacement.java", "FixedPlacement.java",
    "HeightRangePlacement.java", "HeightmapPlacement.java",
    "InSquarePlacement.java", "RandomOffsetPlacement.java",
}
# lighting (#184): core (value/interface layer) + engine (the engines).
LIGHTING_CORE_FILES = {
    "DataLayerStorageMap.java", "DynamicGraphMinFixedPoint.java",
    "LayerLightEventListener.java", "LeveledPriorityQueue.java",
    "LightEventListener.java", "SpatialLongSet.java", "package-info.java",
}

# ---- issue #227: mc.server.level.* class-cluster split ---------------------
SERVER_PKG = "net.minecraft.server.level"
# The residual mc.server.level owns the complement: ServerLevel/ServerPlayer +
# the entity surface + the player/session value types + package-info.
SERVER_RESIDUAL_FILES = {
    "BlockDestructionProgress.java", "ChunkLoadCounter.java",
    "ClientInformation.java", "ColumnPos.java", "DemoMode.java",
    "ParticleStatus.java", "PlayerMap.java", "PlayerSpawnFinder.java",
    "ServerBossEvent.java", "ServerEntity.java", "ServerEntityGetter.java",
    "ServerLevel.java", "ServerPlayer.java", "ServerPlayerGameMode.java",
    "package-info.java",
}
SERVER_CLUSTERS = {
    "mc.server.level.pipeline.chunkmap": {"ChunkMap.java"},
    "mc.server.level.pipeline.holder": {
        "ChunkHolder.java", "GenerationChunkHolder.java",
        "GeneratingChunkMap.java", "ChunkGenerationTask.java",
    },
    "mc.server.level.pipeline.distance": {"DistanceManager.java"},
    "mc.server.level.pipeline.task": {
        "ChunkTaskDispatcher.java", "ChunkTaskPriorityQueue.java",
        "ThrottlingChunkTaskDispatcher.java",
    },
    "mc.server.level.pipeline.level": {
        "ChunkLevel.java", "FullChunkStatus.java", "ChunkResult.java",
    },
    "mc.server.level.pipeline.tracker": {
        "ChunkTracker.java", "LoadingChunkTracker.java",
        "SimulationChunkTracker.java", "SectionTracker.java",
    },
    "mc.server.level.pipeline.view": {"ChunkTrackingView.java"},
    "mc.server.level.pipeline.servercache": {"ServerChunkCache.java"},
    "mc.server.level.pipeline.light": {"ThreadedLevelLightEngine.java"},
    "mc.server.level.pipeline.ticket": {"Ticket.java", "TicketType.java"},
    "mc.server.level.pipeline.region": {"WorldGenRegion.java"},
}
LIGHTING_ENGINE_FILES = {
    "BlockLightEngine.java", "BlockLightSectionStorage.java",
    "ChunkSkyLightSources.java", "LayerLightSectionStorage.java",
    "LevelLightEngine.java", "LightEngine.java", "SkyLightEngine.java",
    "SkyLightSectionStorage.java",
}

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


def main() -> None:
    if not MANIFEST.exists():
        print("MANIFEST.tsv missing; cannot run regression tests")
        return 2

    with tempfile.TemporaryDirectory(prefix="rivet-analyze-test-") as tmp:
        tmpd = Path(tmp)
        # ---- 2. baseline: the committed manifest is reproducible byte-for-byte --
        base = tmpd / "base.tsv"
        run_analyze("--split-nbt", "--split-network", "--split-game",
                    "--split-world", "--split-server", out=base, prev=MANIFEST)
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
        # Every java_paths entry is root-qualified (`root:relpath`, issue #173).
        # Compare multisets so a genuine duplicate would be caught, not just a
        # lost file: the analyzer's global ownership fail-fast guarantees no
        # duplicates ever reach the output, so this pins that property.
        def path_multiset(rs: list[dict]) -> Counter:
            return Counter(p for r in rs for p in r["java_paths"].split(","))

        base_paths = path_multiset(base)
        split_paths = path_multiset(split)
        check("whole manifest: Java inventory conserved (root-qualified, no loss, no duplication)",
              base_paths == split_paths,
              f"base={len(base_paths)} split={len(split_paths)}")

        # ---- 7. root-qualified ownership (issue #173) ----------------------------
        all_tokens = [p for r in base for p in r["java_paths"].split(",")]
        check("every java_paths entry is root-qualified",
              all(p.startswith(("minecraft:", "paper-server:", "paper-api:"))
                  for p in all_tokens))
        check("rooted java_paths have no duplicates",
              len(all_tokens) == len(set(all_tokens)),
              f"{len(all_tokens)} tokens, {len(set(all_tokens))} distinct")

        # The four io.papermc.paper.registry package-info.java pairs physically
        # exist under both paper-api and paper-server; root-qualification makes
        # each a distinct (root, relpath) token instead of a colliding path.
        ROOTS = {
            "minecraft": REPO / "working/Paper/paper-server/src/minecraft/java",
            "paper-server": REPO / "working/Paper/paper-server/src/main/java",
            "paper-api": REPO / "working/Paper/paper-api/src/main/java",
        }
        disk_inventory = {
            f"{root}:{f.relative_to(r).as_posix()}"
            for root, r in ROOTS.items()
            for f in r.rglob("*.java")
        }
        check("rooted java_paths equal the on-disk (root, relpath) inventory",
              set(all_tokens) == disk_inventory,
              f"manifest={len(set(all_tokens))} disk={len(disk_inventory)} "
              f"missing={sorted(disk_inventory - set(all_tokens))[:5]} "
              f"extra={sorted(set(all_tokens) - disk_inventory)[:5]}")
        for unit_id in ("paper.registry.data", "paper.registry.event",
                        "paper.registry.event.type", "paper.registry.set"):
            r = next(x for x in base if x["id"] == unit_id)
            infos = sorted(p for p in r["java_paths"].split(",")
                           if p.endswith("package-info.java"))
            check(f"{unit_id} owns both paper-api: and paper-server: package-info",
                  infos == [
                      f"paper-api:io/papermc/paper/registry/"
                      f"{unit_id.removeprefix('paper.registry.').replace('.', '/')}/"
                      f"package-info.java",
                      f"paper-server:io/papermc/paper/registry/"
                      f"{unit_id.removeprefix('paper.registry.').replace('.', '/')}/"
                      f"package-info.java",
                  ], repr(infos))

        # needs_split is actionable pre-translation state (issue #173): done
        # units lose it on regeneration regardless of graph shape, while the
        # oversized pending residual units keep it and the acyclic oversized
        # versions unit keeps it.
        brigadier = [r for r in base if r["id"].startswith("com.mojang.brigadier")]
        check("done Brigadier units lose needs_split (structural cycle remains in `cycle`)",
              all(r["needs_split"] == "" for r in brigadier)
              and all(r["cycle"] == "3" for r in brigadier))
        versions = next(r for r in base if r["id"] == "ca.spottedleaf.dataconverter.minecraft.versions")
        check("acyclic oversized versions unit keeps needs_split=yes",
              versions["needs_split"] == "yes")
        check("needs_split=yes iff pending/translated and > 15 files",
              all(
                  (r["needs_split"] == "yes") == (r["status"] != "done" and int(r["files"]) > 15)
                  for r in base
              ))

        # Global duplicate-ownership fail-fast: make the scan visit every file
        # twice so each (root, relpath) is declared twice, and require a nonzero
        # exit naming a concrete (root, relpath) token.
        dup_src2 = ANALYZE.read_text(encoding="utf-8").replace(
            "for f in root.rglob(\"*.java\"):",
            "for f in list(root.rglob(\"*.java\")) * 2:",
        ).replace(
            "REPO = Path(__file__).resolve().parent.parent",
            f"REPO = Path({str(REPO)!r})",
        )
        dup2_script = tmpd / "analyze_graph_owndup.py"
        dup2_script.write_text(dup_src2, encoding="utf-8")
        dup2_proc = subprocess.run(
            [sys.executable, str(dup2_script), "--output",
             str(tmpd / "owndup.tsv"), "--prev-manifest", str(MANIFEST)],
            capture_output=True, text=True,
        )
        check("duplicate physical ownership exits nonzero (fail-fast)",
              dup2_proc.returncode != 0)
        check("duplicate physical ownership names a concrete root:relpath and both units",
              "duplicate physical ownership:" in dup2_proc.stderr
              and any(prefix in dup2_proc.stderr
                      for prefix in ("minecraft:", "paper-server:", "paper-api:"))
              and "is declared in both" in dup2_proc.stderr
              and ".java" in dup2_proc.stderr)

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
                    "--split-world", "--split-server", out=both, prev=MANIFEST)
        both_rows = rows_of(both)
        both_ids = {r["id"] for r in both_rows}
        check("all flags compose: nbt + network + game + world split units present",
              {"mc.nbt", "mc.nbt.snbt", "mc.network", "mc.network.buf",
               "mc.network.framing", "mc.network.protocol.game",
               "mc.network.protocol.game.join", "mc.network.protocol.game.chunk",
               "mc.network.protocol.game.serverbound",
               "mc.world.level.levelgen.random", "mc.world.level.levelgen.noise",
               "mc.world.level.levelgen.noisegen",
               "mc.world.level.levelgen.surface",
               "mc.world.level.levelgen.feature.core",
               "mc.world.level.biome.core", "mc.world.level.chunk.wire",
               "mc.world.level.levelgen.blockpredicates.core",
               "mc.world.level.levelgen.placement.core",
               "mc.world.level.lighting.engine"} <= both_ids)
        check("all flags compose: server split units present",
              {"mc.server.level", "mc.server.level.pipeline.chunkmap",
               "mc.server.level.pipeline.holder",
               "mc.server.level.pipeline.distance",
               "mc.server.level.pipeline.task",
               "mc.server.level.pipeline.level",
               "mc.server.level.pipeline.tracker",
               "mc.server.level.pipeline.view",
               "mc.server.level.pipeline.servercache",
               "mc.server.level.pipeline.light",
               "mc.server.level.pipeline.ticket",
               "mc.server.level.pipeline.region"} <= both_ids)
        check("all flags compose: world residuals present where expected",
              {"mc.world.level.levelgen.structure",
               "mc.world.level.levelgen.structure.structures",
               "mc.world.level.storage"} <= both_ids)
        check("all flags compose: templatesystem is fully partitioned (no residual)",
              "mc.world.level.levelgen.structure.templatesystem" not in both_ids
              and {"mc.world.level.levelgen.structure.templatesystem.core",
                   "mc.world.level.levelgen.structure.templatesystem.rules",
                   "mc.world.level.levelgen.structure.templatesystem.processors"}
              <= both_ids)
        check("all flags compose: chunk is fully partitioned (no residual)",
              "mc.world.level.chunk" not in both_ids
              and {"mc.world.level.chunk.wire", "mc.world.level.chunk.access",
                   "mc.world.level.chunk.support", "mc.world.level.chunk.generator"}
              <= both_ids)
        check("all flags compose: fully-partitioned feature leaves present",
              {"mc.world.level.levelgen.feature.core",
               "mc.world.level.levelgen.feature.tree",
               "mc.world.level.levelgen.feature.bamboo",
               "mc.world.level.levelgen.feature.configurations.core",
               "mc.world.level.levelgen.feature.configurations.ore",
               "mc.world.level.levelgen.spawner"} <= both_ids)

        # ---- 3c. world split (#176): right-sized class-cluster units ---------------
        world1 = tmpd / "world1.tsv"
        world2 = tmpd / "world2.tsv"
        run_analyze("--split-world", out=world1, prev=MANIFEST)
        run_analyze("--split-world", out=world2, prev=world1)
        check("world split: two regenerations byte-identical",
              world1.read_bytes() == world2.read_bytes())
        wsplit = rows_of(world1)
        w_by_id = {r["id"]: r for r in wsplit}
        w_by_pkg: dict[str, list[dict]] = {}
        for r in wsplit:
            w_by_pkg.setdefault(r["java_package"], []).append(r)
        w_file_set = lambda r: {p.rsplit("/", 1)[-1] for p in r["java_paths"].split(",")}  # noqa: E731

        # Fully-partitioned packages (levelgen, biome, feature, configurations,
        # chunk) emit no residual row and their pre-split ids disappear from the
        # manifest.
        check("levelgen/biome/feature/configs/chunk are fully partitioned (no residual row)",
              all(derive_id(p) not in w_by_id for p in FULLY_PARTITIONED))
        check("levelgen root splits into exactly random/noise/noisegen/settings/surface/spawner",
              {r["id"] for r in w_by_pkg[LEVELGEN_PKG]}
              == {"mc.world.level.levelgen.random", "mc.world.level.levelgen.noise",
                  "mc.world.level.levelgen.noisegen",
                  "mc.world.level.levelgen.settings", "mc.world.level.levelgen.surface",
                  "mc.world.level.levelgen.spawner"})
        check("levelgen.noise owns the density/noise-router value layer (#177)",
              w_file_set(w_by_id["mc.world.level.levelgen.noise"])
              == LEVELGEN_NOISE_FILES)
        check("levelgen.noisegen owns the noise-based chunk generator SCC (#183/#185)",
              w_file_set(w_by_id["mc.world.level.levelgen.noisegen"])
              == LEVELGEN_NOISEGEN_FILES)
        check("noise is right-sized (no needs_split) and noisegen rides on noise",
              w_by_id["mc.world.level.levelgen.noise"]["needs_split"] == ""
              and "mc.world.level.levelgen.noise" in w_by_id["mc.world.level.levelgen.noisegen"]["deps"].split(","))
        check("spawner owns the two CustomSpawner impls (not surface)",
              w_file_set(w_by_id["mc.world.level.levelgen.spawner"])
              == {"PatrolSpawner.java", "PhantomSpawner.java"}
              and "PatrolSpawner.java" not in w_file_set(w_by_id["mc.world.level.levelgen.surface"]))
        check("biome splits into exactly core/source/data",
              {r["id"] for r in w_by_pkg[BIOME_PKG]}
              == {"mc.world.level.biome.core", "mc.world.level.biome.source",
                  "mc.world.level.biome.data"})
        check("levelgen.random owns the 10 RNG classes",
              w_file_set(w_by_id["mc.world.level.levelgen.random"])
              == LEVELGEN_RANDOM_FILES)
        # Feature residual owns the complement (all 75 feature files minus the
        # 35 in the authored clusters).
        feature_units = [r for r in wsplit if r["java_package"] == FEATURE_PKG]
        feature_owned = set().union(*[w_file_set(r) for r in feature_units])
        feature_dir = (REPO / "working/Paper/paper-server/src/minecraft/java"
                       / "net/minecraft/world/level/levelgen/feature")
        check("feature package: every on-disk *.java owned exactly once",
              feature_dir.is_dir()
              and feature_owned == {p.name for p in feature_dir.glob("*.java")}
              and len(feature_units) == len({r["id"] for r in feature_units}),
              f"units={len(feature_units)}")
        check("feature.core owns exactly the hub + context + package-info",
              w_file_set(w_by_id["mc.world.level.levelgen.feature.core"])
              == FEATURE_CORE_FILES)
        check("structures residual holds the M3-deferred pairs + package-info",
              w_file_set(w_by_id[derive_id(STRUCTURES_PKG)])
              == {"EndCityPieces.java", "EndCityStructure.java",
                  "NetherFortressPieces.java", "NetherFortressStructure.java",
                  "NetherFossilPieces.java", "NetherFossilStructure.java",
                  "package-info.java"})
        check("structures authored pairs are disjoint from the residual",
              STRUCTURES_FILES.isdisjoint(
                  w_file_set(w_by_id[derive_id(STRUCTURES_PKG)])))
        check("structure.framework owns the base SCCs + StructureSet/BoundingBox",
              w_file_set(w_by_id["mc.world.level.levelgen.structure.framework"])
              == STRUCTURE_FRAMEWORK_FILES)
        check("structure.framework.pieces owns the concrete piece base classes",
              w_file_set(w_by_id["mc.world.level.levelgen.structure.framework.pieces"])
              == STRUCTURE_FRAMEWORK_PIECES_FILES)
        check("structure.framework is right-sized (no needs_split)",
              w_by_id["mc.world.level.levelgen.structure.framework"]["needs_split"] == "")
        check("templatesystem.core owns the processor framework (8 files)",
              w_file_set(w_by_id["mc.world.level.levelgen.structure.templatesystem.core"])
              == TEMPLATESYSTEM_CORE_FILES)
        check("templatesystem.rules owns the RuleTest/PosRuleTest families",
              w_file_set(w_by_id["mc.world.level.levelgen.structure.templatesystem.rules"])
              == TEMPLATESYSTEM_RULES_FILES)
        check("templatesystem.processors owns the concrete processors + hub",
              w_file_set(w_by_id["mc.world.level.levelgen.structure.templatesystem.processors"])
              == TEMPLATESYSTEM_PROCESSORS_FILES)
        check("templatesystem is fully partitioned and processors -> core + rules",
              derive_id(TEMPLATESYSTEM_PKG) not in w_by_id
              and "mc.world.level.levelgen.structure.templatesystem.rules"
              in w_by_id["mc.world.level.levelgen.structure.templatesystem.processors"]["deps"].split(","))
        # blockpredicates (#180): core + combinators/states/simple, all right-sized.
        check("blockpredicates splits into core + combinators/states/simple",
              {r["id"] for r in w_by_pkg[BLOCKPREDICATES_PKG]}
              == {"mc.world.level.levelgen.blockpredicates.core",
                  "mc.world.level.levelgen.blockpredicates.combinators",
                  "mc.world.level.levelgen.blockpredicates.states",
                  "mc.world.level.levelgen.blockpredicates.simple"})
        check("blockpredicates.core owns the hub + combinators/state base",
              w_file_set(w_by_id["mc.world.level.levelgen.blockpredicates.core"])
              == BLOCKPREDICATES_CORE_FILES)
        check("blockpredicates leaves own their authored files and -> core",
              w_file_set(w_by_id["mc.world.level.levelgen.blockpredicates.combinators"])
              == BLOCKPREDICATES_COMBINATORS_FILES
              and "mc.world.level.levelgen.blockpredicates.core"
              in w_by_id["mc.world.level.levelgen.blockpredicates.combinators"]["deps"].split(",")
              and w_file_set(w_by_id["mc.world.level.levelgen.blockpredicates.states"])
              == BLOCKPREDICATES_STATES_FILES
              and w_file_set(w_by_id["mc.world.level.levelgen.blockpredicates.simple"])
              == BLOCKPREDICATES_SIMPLE_FILES)
        check("blockpredicates leaves are not needs_split",
              all(w_by_id[u]["needs_split"] == "" for u in
                  ("mc.world.level.levelgen.blockpredicates.core",
                   "mc.world.level.levelgen.blockpredicates.combinators",
                   "mc.world.level.levelgen.blockpredicates.states",
                   "mc.world.level.levelgen.blockpredicates.simple")))
        # placement (#181): core + repeating/filter/simple, all right-sized.
        check("placement splits into core + repeating/filter/simple",
              {r["id"] for r in w_by_pkg[PLACEMENT_PKG]}
              == {"mc.world.level.levelgen.placement.core",
                  "mc.world.level.levelgen.placement.repeating",
                  "mc.world.level.levelgen.placement.filter",
                  "mc.world.level.levelgen.placement.simple"})
        check("placement.core owns the modifier base + registry hub",
              w_file_set(w_by_id["mc.world.level.levelgen.placement.core"])
              == PLACEMENT_CORE_FILES)
        check("placement leaves own their authored files and -> core",
              w_file_set(w_by_id["mc.world.level.levelgen.placement.repeating"])
              == PLACEMENT_REPEATING_FILES
              and "mc.world.level.levelgen.placement.core"
              in w_by_id["mc.world.level.levelgen.placement.repeating"]["deps"].split(",")
              and w_file_set(w_by_id["mc.world.level.levelgen.placement.filter"])
              == PLACEMENT_FILTER_FILES
              and w_file_set(w_by_id["mc.world.level.levelgen.placement.simple"])
              == PLACEMENT_SIMPLE_FILES)
        check("placement leaves are not needs_split",
              all(w_by_id[u]["needs_split"] == "" for u in
                  ("mc.world.level.levelgen.placement.core",
                   "mc.world.level.levelgen.placement.repeating",
                   "mc.world.level.levelgen.placement.filter",
                   "mc.world.level.levelgen.placement.simple")))
        # lighting (#184): core (value/interface layer) + engine.
        check("lighting splits into core + engine",
              {r["id"] for r in w_by_pkg[LIGHTING_PKG]}
              == {"mc.world.level.lighting.core", "mc.world.level.lighting.engine"})
        check("lighting.core owns the value/interface layer",
              w_file_set(w_by_id["mc.world.level.lighting.core"])
              == LIGHTING_CORE_FILES)
        check("lighting.engine owns the engines + section storages and -> core",
              w_file_set(w_by_id["mc.world.level.lighting.engine"])
              == LIGHTING_ENGINE_FILES
              and "mc.world.level.lighting.core"
              in w_by_id["mc.world.level.lighting.engine"]["deps"].split(","))
        check("lighting clusters are not needs_split",
              all(w_by_id[u]["needs_split"] == "" for u in
                  ("mc.world.level.lighting.core", "mc.world.level.lighting.engine")))
        check("chunk.wire owns the palette-container value layer + Strategy + package-info",
              w_file_set(w_by_id["mc.world.level.chunk.wire"]) == CHUNK_WIRE_FILES)
        check("chunk.access owns the #183 in-memory chunk data SCC + ChunkSource + UpgradeData",
              w_file_set(w_by_id["mc.world.level.chunk.access"]) == CHUNK_ACCESS_FILES)
        check("chunk.support owns the dependency-free leaf types",
              w_file_set(w_by_id["mc.world.level.chunk.support"]) == CHUNK_SUPPORT_FILES)
        check("chunk.generator owns the #185 generator stack",
              w_file_set(w_by_id["mc.world.level.chunk.generator"]) == CHUNK_GENERATOR_FILES)
        # Every on-disk chunk *.java is owned exactly once across the four
        # fully-partitioned clusters (checked against disk when working/ is
        # present).
        chunk_dir = (REPO / "working/Paper/paper-server/src/minecraft/java"
                     / "net/minecraft/world/level/chunk")
        if chunk_dir.is_dir():
            chunk_owned = set().union(
                *[w_file_set(w_by_id[u]) for u in
                  ("mc.world.level.chunk.wire", "mc.world.level.chunk.access",
                   "mc.world.level.chunk.support", "mc.world.level.chunk.generator")])
            check("chunk package: every on-disk *.java owned exactly once",
                  chunk_owned == {p.name for p in chunk_dir.glob("*.java")})
        # storage: the residual keeps the pre-split id and owns the
        # LevelStorageSource hub; the four foundation clusters + primaryleveldata
        # own exactly their authored files.
        storage_units = [r for r in wsplit if r["java_package"] == STORAGE_PKG]
        check("storage splits into residual + version/leveldata/primaryleveldata/saveddata/value",
              {r["id"] for r in storage_units}
              == {"mc.world.level.storage", "mc.world.level.storage.version",
                  "mc.world.level.storage.leveldata",
                  "mc.world.level.storage.primaryleveldata",
                  "mc.world.level.storage.saveddata", "mc.world.level.storage.value"})
        check("storage residual owns the LevelStorageSource hub + package-info",
              w_file_set(w_by_id["mc.world.level.storage"]) == STORAGE_RESIDUAL_FILES)
        check("storage.version owns DataVersion/LevelVersion",
              w_file_set(w_by_id["mc.world.level.storage.version"])
              == STORAGE_VERSION_FILES)
        check("storage.leveldata owns the LevelData interface family",
              w_file_set(w_by_id["mc.world.level.storage.leveldata"])
              == STORAGE_LEVELDATA_FILES)
        check("storage.primaryleveldata owns PrimaryLevelData + DerivedLevelData + LevelDataAndDimensions",
              w_file_set(w_by_id["mc.world.level.storage.primaryleveldata"])
              == STORAGE_PRIMARY_FILES)
        check("storage.saveddata owns SavedDataStorage + CommandStorage",
              w_file_set(w_by_id["mc.world.level.storage.saveddata"])
              == STORAGE_SAVEDDATA_FILES)
        check("storage.value owns the NBT tag value codec family",
              w_file_set(w_by_id["mc.world.level.storage.value"])
              == STORAGE_VALUE_FILES)
        # The fully-partitioned levelgen/biome/chunk pre-split rows vanish; their
        # files must still be owned once. Compare root-qualified inventory
        # multiset against the flat base manifest.
        wsplit_paths = Counter(
            p for r in wsplit if r["java_package"] in WORLD_PACKAGES
            for p in r["java_paths"].split(",")
        )
        base_world_paths = Counter(
            p for r in base
            if r["java_package"] in WORLD_PACKAGES
            for p in r["java_paths"].split(",")
        )
        check("world split: Java inventory conserved across the 12 packages",
              wsplit_paths == base_world_paths,
              f"base={len(base_world_paths)} split={len(wsplit_paths)}")
        # Source root preserved: every world unit stays `minecraft`.
        world_units = [r for r in wsplit if r["java_package"] in WORLD_PACKAGES]
        check("world split: source_root preserved (minecraft)",
              all(r["source_root"] == "minecraft" for r in world_units))

        # wave/cycle preserved: all world units stay wave=3 / cycle=27 (they
        # remain inside the giant SCC; the split right-sizes ownership).
        check("world split: wave preserved (all wave=3)",
              all(int(r["wave"]) == 3 for r in world_units))
        check("world split: cycle preserved (all cycle=27)",
              all(r["cycle"] == "27" for r in world_units))

        # The oversized residuals keep needs_split=yes; the right-sized clusters
        # and fully-partitioned leaves do not.
        check("world residual structure keeps needs_split=yes and storage residual is right-sized",
              w_by_id["mc.world.level.levelgen.structure"]["needs_split"] == ""
              and w_by_id["mc.world.level.storage"]["needs_split"] == "")
        # Every right-sized cluster (< 15 files) is unflagged — the split is an
        # ownership right-size, not a claim of acyclicity: every created world
        # cluster is now under the SPLIT_FILE_THRESHOLD, and the only oversized
        # world.level rows that keep needs_split=yes are the un-split monolithic
        # packages owned by other epics (block, block.entity, block.state.
        # properties, entity, storage.loot.* — each tracked separately).
        check("world clusters are not needs_split",
              all(w_by_id[u]["needs_split"] == "" for u in
                  ("mc.world.level.levelgen.random",
                   "mc.world.level.levelgen.noise",
                   "mc.world.level.levelgen.noisegen",
                   "mc.world.level.levelgen.settings",
                   "mc.world.level.levelgen.surface",
                   "mc.world.level.levelgen.spawner",
                   "mc.world.level.levelgen.feature.core",
                   "mc.world.level.levelgen.feature.tree",
                   "mc.world.level.levelgen.feature.bamboo",
                   "mc.world.level.levelgen.feature.configurations.core",
                   "mc.world.level.levelgen.feature.configurations.ore",
                   "mc.world.level.levelgen.blockpredicates.core",
                   "mc.world.level.levelgen.placement.core",
                   "mc.world.level.levelgen.structure.framework",
                   "mc.world.level.levelgen.structure.framework.pieces",
                   "mc.world.level.levelgen.structure.templatesystem.core",
                   "mc.world.level.levelgen.structure.templatesystem.rules",
                   "mc.world.level.levelgen.structure.templatesystem.processors",
                   "mc.world.level.levelgen.structure.structures.stronghold",
                   "mc.world.level.biome.core", "mc.world.level.chunk.wire",
                   "mc.world.level.chunk.access", "mc.world.level.chunk.support",
                   "mc.world.level.chunk.generator", "mc.world.level.storage",
                   "mc.world.level.storage.version", "mc.world.level.storage.leveldata",
                   "mc.world.level.storage.primaryleveldata",
                   "mc.world.level.storage.saveddata", "mc.world.level.storage.value",
                   "mc.world.level.lighting.core", "mc.world.level.lighting.engine")))

        # data.worldgen (#177/#178 prereq): the 3-file bootstrap/terrain slice
        # splits out of the monolithic row; the residual keeps the pre-split id
        # (so the data rows in wave 3's cycle still resolve to one hub) and the
        # 26-file complement, and stays needs_split=yes.
        check("data.worldgen splits into residual + prereq",
              {r["id"] for r in w_by_pkg[DATA_WORLDGEN_PKG]}
              == {"mc.data.worldgen", "mc.data.worldgen.prereq"})
        check("data.worldgen.prereq owns the bootstrap/terrain slice",
              w_file_set(w_by_id["mc.data.worldgen.prereq"])
              == DATA_WORLDGEN_PREREQ_FILES)
        check("data.worldgen residual owns the 26-file complement",
              w_file_set(w_by_id["mc.data.worldgen"]) == DATA_WORLDGEN_RESIDUAL_FILES
              and DATA_WORLDGEN_PREREQ_FILES.isdisjoint(
                  w_file_set(w_by_id["mc.data.worldgen"])))
        check("data.worldgen.prereq is right-sized (no needs_split) and rides on the residual",
              w_by_id["mc.data.worldgen.prereq"]["needs_split"] == ""
              and w_by_id["mc.data.worldgen"]["needs_split"] == "yes"
              and "mc.data.worldgen.prereq"
              in w_by_id["mc.data.worldgen"]["deps"].split(","))
        check("data.worldgen.prereq crate-overrides to rivet-world",
              w_by_id["mc.data.worldgen.prereq"]["crate"] == "rivet-world"
              and w_by_id["mc.data.worldgen"]["crate"] == "rivet-registry")
        data_dir = (REPO / "working/Paper/paper-server/src/minecraft/java"
                    / "net/minecraft/data/worldgen")
        if data_dir.is_dir():
            data_owned = set().union(
                *[w_file_set(w_by_id[u]) for u in
                  ("mc.data.worldgen", "mc.data.worldgen.prereq")])
            check("data.worldgen package: every on-disk *.java owned exactly once",
                  data_owned == {p.name for p in data_dir.glob("*.java")})

        # All world dep tokens resolve via the wave-picker rules (unit id,
        # derived package id, or lowest-id shared-package row).
        w_unresolved = []
        for r in wsplit:
            for tok in (t.strip() for t in r["deps"].split(",") if t.strip()):
                if resolve_dep(tok, w_by_id, w_by_pkg) is None:
                    w_unresolved.append((r["id"], tok))
        check("all dep tokens in the world split manifest resolve to a unit",
              not w_unresolved, w_unresolved[:5])

        # Same-package intra-cluster edges are authored as unit ids in
        # SPLIT_EDGES; pin the FULL edge set so a dropped or stale edge cannot
        # silently change wave-sequencing. The residual -> sub-unit edges are
        # the residual rows' deps; the leaf -> core star is the family/leaf
        # clusters' deps. Every edge must appear in the emitted all-flags
        # manifest (network/game edges only exist under their flags).
        from analyze_graph import SPLIT_EDGES
        b_by_id = {r["id"]: r for r in both_rows}
        b_by_pkg: dict[str, list[dict]] = {}
        for r in both_rows:
            b_by_pkg.setdefault(r["java_package"], []).append(r)
        for src, targets in sorted(SPLIT_EDGES.items()):
            for tgt in sorted(targets):
                check(f"SPLIT_EDGES: {src} -> {tgt} present in manifest",
                      src in b_by_id
                      and tgt in b_by_id[src]["deps"].split(","),
                      f"missing edge {src}->{tgt}")
        # CRATE_OVERRIDES must name real emitted split units (a dropped or stale
        # override would silently misroute a wave to the wrong crate).
        from analyze_graph import CRATE_OVERRIDES
        for unit_id, crate in sorted(CRATE_OVERRIDES.items()):
            check(f"CRATE_OVERRIDES: {unit_id} is an emitted unit",
                  unit_id in b_by_id,
                  f"override names unknown unit {unit_id}")
            check(f"CRATE_OVERRIDES: {unit_id} crate is {crate}",
                  b_by_id[unit_id]["crate"] == crate,
                  f"{unit_id} crate is {b_by_id[unit_id]['crate']}, want {crate}")
        # And the reverse: no extra authored same-package edges beyond the set.
        # (The nbt split authors its same-package unit-id deps in NBT_UNITS, not
        # SPLIT_EDGES, so it is excluded here.)
        for unit_id, row in b_by_id.items():
            if row["java_package"].startswith("net.minecraft.nbt"):
                continue
            for t in (x.strip() for x in row["deps"].split(",") if x.strip()):
                if t in b_by_id and row["java_package"] == b_by_id[t]["java_package"]:
                    check(f"SPLIT_EDGES: no spurious same-package edge {unit_id}->{t}",
                          t in SPLIT_EDGES.get(unit_id, ()),
                          f"unexpected same-package dep {unit_id}->{t}")

        # Authored STUB notes carry into the notes column for the cross-wave
        # back-edges (noisegen -> surface/settings) and the generated hubs.
        check("noisegen unit carries the surface/settings STUB note",
              "M2 STUB:" in w_by_id["mc.world.level.levelgen.noisegen"]["notes"])
        check("noise unit carries the #177 wave note",
              "#177" in w_by_id["mc.world.level.levelgen.noise"]["notes"])
        check("feature.core carries the generated-hub note",
              "generated content" in w_by_id["mc.world.level.levelgen.feature.core"]["notes"])
        check("blockpredicates.core carries the generated-hub note",
              "generated content" in w_by_id["mc.world.level.levelgen.blockpredicates.core"]["notes"])
        check("placement.core carries the generated-hub note",
              "generated content" in w_by_id["mc.world.level.levelgen.placement.core"]["notes"])

        # Carry across regeneration: seed a world unit status, regenerate, and
        # verify it survives alongside the authored note.
        w_seeded = tmpd / "w_seeded.tsv"
        w_carry_rows = []
        for r in wsplit:
            if r["id"] in ("mc.world.level.levelgen.random",
                           "mc.world.level.levelgen.feature.bamboo"):
                r["status"] = "translated"
                r["attempts"] = "2"
                r["notes"] = "random-wave note"
            w_carry_rows.append(r)
        with w_seeded.open("w", encoding="utf-8") as fh:
            fh.write("\t".join(w_carry_rows[0].keys()) + "\n")
            for r in w_carry_rows:
                fh.write("\t".join(r.values()) + "\n")
        w_regen = tmpd / "w_regen.tsv"
        run_analyze("--split-world", out=w_regen, prev=w_seeded)
        w_regen_rows = rows_of(w_regen)
        w_regen_by_id = {r["id"]: r for r in w_regen_rows}
        wr = w_regen_by_id["mc.world.level.levelgen.random"]
        check("world carry: random keeps status/attempts/notes",
              wr["status"] == "translated" and wr["attempts"] == "2"
              and "random-wave note" in wr["notes"])
        wb = w_regen_by_id["mc.world.level.levelgen.feature.bamboo"]
        check("world carry: feature leaf keeps status/attempts/notes",
              wb["status"] == "translated" and wb["attempts"] == "2"
              and "random-wave note" in wb["notes"])

        # ---- 3d. server split (#227): right-sized class clusters -----------------
        server1 = tmpd / "server1.tsv"
        server2 = tmpd / "server2.tsv"
        run_analyze("--split-server", out=server1, prev=MANIFEST)
        run_analyze("--split-server", out=server2, prev=server1)
        check("server split: two regenerations byte-identical",
              server1.read_bytes() == server2.read_bytes())
        ssplit = rows_of(server1)
        s_by_id = {r["id"]: r for r in ssplit}
        s_by_pkg: dict[str, list[dict]] = {}
        for r in ssplit:
            s_by_pkg.setdefault(r["java_package"], []).append(r)
        s_file_set = lambda r: {p.rsplit("/", 1)[-1] for p in r["java_paths"].split(",")}  # noqa: E731

        # The residual keeps the pre-split id; the 11 authored clusters split off.
        server_units = [r for r in ssplit if r["java_package"] == SERVER_PKG]
        check("server split: exactly 12 units (residual + 11 clusters)",
              sorted(r["id"] for r in server_units) == sorted(
                  ["mc.server.level"] + list(SERVER_CLUSTERS)))
        check("server split: source_root preserved (minecraft)",
              all(r["source_root"] == "minecraft" for r in server_units))
        # Every authored cluster owns exactly its file list.
        for unit_id, files in sorted(SERVER_CLUSTERS.items()):
            check(f"server cluster {unit_id} owns exactly its authored files",
                  s_file_set(s_by_id[unit_id]) == files)
        # The residual owns the complement (never an authored file, and together
        # with the clusters every on-disk file is owned exactly once).
        check("server residual owns the complement (no authored file in it)",
              s_file_set(s_by_id["mc.server.level"]) == SERVER_RESIDUAL_FILES)
        server_dir = (REPO / "working/Paper/paper-server/src/minecraft/java"
                      / "net/minecraft/server/level")
        if server_dir.is_dir():
            owned = set()
            dup = set()
            for r in server_units:
                for f in s_file_set(r):
                    (dup if f in owned else owned).add(f)
            check("server package: every on-disk *.java owned exactly once",
                  owned == {p.name for p in server_dir.glob("*.java")} and not dup,
                  f"owned={len(owned)} dup={sorted(dup)}")
        # The residual is right-sized (< 15 files), so needs_split is cleared.
        check("server clusters and residual are not needs_split",
              all(s_by_id[u]["needs_split"] == "" for u in
                  ["mc.server.level"] + list(SERVER_CLUSTERS)))
        # wave/cycle preserved: every unit stays in the giant SCC (wave=3,
        # cycle=27) — the split right-sizes ownership, it does not claim
        # acyclicity.
        check("server split: wave preserved (all wave=3)",
              all(int(r["wave"]) == 3 for r in server_units))
        check("server split: cycle preserved (all cycle=27)",
              all(r["cycle"] == "27" for r in server_units))
        # All server dep tokens resolve via the wave-picker rules.
        s_unresolved = []
        for r in ssplit:
            for tok in (t.strip() for t in r["deps"].split(",") if t.strip()):
                if resolve_dep(tok, s_by_id, s_by_pkg) is None:
                    s_unresolved.append((r["id"], tok))
        check("all dep tokens in the server split manifest resolve to a unit",
              not s_unresolved, s_unresolved[:5])
        # The residual carries STUB notes for the 11 cluster back-references into
        # the untranslated tail; each cluster names its issue + STUBs.
        check("server residual carries the #227 residual note",
              "#227 residual" in s_by_id["mc.server.level"]["notes"])
        for unit_id in ("mc.server.level.pipeline.chunkmap",
                        "mc.server.level.pipeline.holder",
                        "mc.server.level.pipeline.distance",
                        "mc.server.level.pipeline.servercache",
                        "mc.server.level.pipeline.light",
                        "mc.server.level.pipeline.region"):
            check(f"server cluster {unit_id} carries a #185 STUB note",
                  "M2 STUB:" in s_by_id[unit_id]["notes"])
        # Server-split carry: seed the residual + one cluster, regenerate with
        # --split-server, and verify status/attempts/notes survive alongside the
        # authored note.
        s_seeded = tmpd / "s_seeded.tsv"
        s_carry_rows = []
        for r in ssplit:
            if r["id"] in ("mc.server.level", "mc.server.level.pipeline.chunkmap"):
                r["status"] = "translated"
                r["attempts"] = "2"
                r["notes"] = "pipeline-wave note"
            s_carry_rows.append(r)
        with s_seeded.open("w", encoding="utf-8") as fh:
            fh.write("\t".join(s_carry_rows[0].keys()) + "\n")
            for r in s_carry_rows:
                fh.write("\t".join(r.values()) + "\n")
        s_regen = tmpd / "s_regen.tsv"
        run_analyze("--split-server", out=s_regen, prev=s_seeded)
        s_regen_rows = rows_of(s_regen)
        s_regen_by_id = {r["id"]: r for r in s_regen_rows}
        for unit_id in ("mc.server.level", "mc.server.level.pipeline.chunkmap"):
            r = s_regen_by_id[unit_id]
            check(f"server carry: {unit_id} keeps status/attempts/notes",
                  r["status"] == "translated" and r["attempts"] == "2"
                  and "pipeline-wave note" in r["notes"])
        check("server carry: residual keeps authored #227 note",
              "#227 residual" in s_regen_by_id["mc.server.level"]["notes"])
        check("server carry: chunkmap keeps authored STUB note",
              "M2 STUB:" in s_regen_by_id["mc.server.level.pipeline.chunkmap"]["notes"])
        # Residual durability: seeding durable state on the flat mc.server.level
        # row survives onto the residual (which keeps the pre-split id), and
        # every external dependent still resolves net.minecraft.server.level to
        # the hub — not to the lowest-id cluster.
        s_flat = tmpd / "s_flat.tsv"
        run_analyze(out=s_flat, prev=MANIFEST)
        s_state = tmpd / "s_state.tsv"
        s_rows = list(csv.DictReader(s_flat.open(newline=""), delimiter="\t"))
        for r in s_rows:
            if r["id"] == "mc.server.level":
                r["status"] = "translated"
                r["attempts"] = "2"
                r["notes"] = "server-residual wave note"
        with s_state.open("w", encoding="utf-8") as fh:
            fh.write("\t".join(s_rows[0].keys()) + "\n")
            for r in s_rows:
                fh.write("\t".join(r.values()) + "\n")
        s_carry = tmpd / "s_carry.tsv"
        run_analyze("--split-server", out=s_carry, prev=s_state)
        s_carry_rows = rows_of(s_carry)
        s_carry_by_id = {r["id"]: r for r in s_carry_rows}
        sr = s_carry_by_id["mc.server.level"]
        check("server residual carry: status/attempts/notes survive onto the residual",
              sr["status"] == "translated" and sr["attempts"] == "2"
              and "server-residual wave note" in sr["notes"])
        s_carry_by_pkg: dict[str, list[dict]] = {}
        for r in s_carry_rows:
            s_carry_by_pkg.setdefault(r["java_package"], []).append(r)
        s_misresolved = []
        for r in s_carry_rows:
            for tok in (t.strip() for t in r["deps"].split(",") if t.strip()):
                if tok == "net.minecraft.server.level":
                    if resolve_dep(tok, s_carry_by_id, s_carry_by_pkg)["id"] != "mc.server.level":
                        s_misresolved.append(r["id"])
        check("server residual carry: all dependents resolve server.level token to the residual",
              not s_misresolved, s_misresolved[:5])
        # Server-split inventory conserved against the flat manifest: the
        # root-qualified multiset over the package is unchanged.
        ssplit_paths = Counter(
            p for r in ssplit if r["java_package"] == SERVER_PKG
            for p in r["java_paths"].split(",")
        )
        base_server_paths = Counter(
            p for r in base if r["java_package"] == SERVER_PKG
            for p in r["java_paths"].split(",")
        )
        check("server split: Java inventory conserved",
              ssplit_paths == base_server_paths,
              f"base={len(base_server_paths)} split={len(ssplit_paths)}")

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
            '            "ServerboundMovePlayerPacket.java",\n'
            '            "ServerboundChunkBatchReceivedPacket.java",',
            '            "ServerboundMovePlayerPacket.java",\n'
            '            "ServerboundChunkBatchReceivedPacket.java",\n'
            '            "ClientboundGameEventPacket.java",',
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

        # World-split fail-fast: declare NoiseBasedChunkGenerator.java in both
        # the noise and surface clusters and require a nonzero exit naming both
        # owning units.
        wdup_src = ANALYZE.read_text(encoding="utf-8").replace(
            '            "SurfaceRules.java", "SurfaceSystem.java",\n'
            '        ],',
            '            "SurfaceRules.java", "SurfaceSystem.java",\n'
            '            "NoiseBasedChunkGenerator.java",\n'
            '        ],',
        ).replace(
            "REPO = Path(__file__).resolve().parent.parent",
            f"REPO = Path({str(REPO)!r})",
        )
        wdup_script = tmpd / "analyze_graph_wdup.py"
        wdup_script.write_text(wdup_src, encoding="utf-8")
        wdup_proc = subprocess.run(
            [sys.executable, str(wdup_script), "--split-world",
             "--output", str(tmpd / "wdup.tsv"), "--prev-manifest", str(MANIFEST)],
            capture_output=True, text=True,
        )
        check("world duplicate declaration exits nonzero (fail-fast)",
              wdup_proc.returncode != 0)
        check("world duplicate declaration names both owning units",
              "NoiseBasedChunkGenerator.java is declared in both "
              "mc.world.level.levelgen.noisegen and "
              "mc.world.level.levelgen.surface" in wdup_proc.stderr)

        # Server-split fail-fast: declare ChunkMap.java in both the chunkmap and
        # holder clusters and require a nonzero exit naming both owning units.
        sdup_src = ANALYZE.read_text(encoding="utf-8").replace(
            '            "ChunkHolder.java", "GenerationChunkHolder.java",\n'
            '            "GeneratingChunkMap.java", "ChunkGenerationTask.java",',
            '            "ChunkHolder.java", "GenerationChunkHolder.java",\n'
            '            "GeneratingChunkMap.java", "ChunkGenerationTask.java",\n'
            '            "ChunkMap.java",',
        ).replace(
            "REPO = Path(__file__).resolve().parent.parent",
            f"REPO = Path({str(REPO)!r})",
        )
        sdup_script = tmpd / "analyze_graph_sdup.py"
        sdup_script.write_text(sdup_src, encoding="utf-8")
        sdup_proc = subprocess.run(
            [sys.executable, str(sdup_script), "--split-server",
             "--output", str(tmpd / "sdup.tsv"), "--prev-manifest", str(MANIFEST)],
            capture_output=True, text=True,
        )
        check("server duplicate declaration exits nonzero (fail-fast)",
              sdup_proc.returncode != 0)
        check("server duplicate declaration names both owning units",
              "ChunkMap.java is declared in both "
              "mc.server.level.pipeline.chunkmap and "
              "mc.server.level.pipeline.holder" in sdup_proc.stderr)

        # Planned-id uniqueness (A3): a sub-unit id that shadows a package-
        # derived row must fail fast, not silently clobber. Simulate a cluster
        # id collision with an existing sub-package row.
        idup_src = ANALYZE.read_text(encoding="utf-8").replace(
            '    "mc.world.level.levelgen.random": [',
            '    "mc.world.level.levelgen.synth": [',
            1,
        ).replace(
            "REPO = Path(__file__).resolve().parent.parent",
            f"REPO = Path({str(REPO)!r})",
        )
        idup_script = tmpd / "analyze_graph_idup.py"
        idup_script.write_text(idup_src, encoding="utf-8")
        idup_proc = subprocess.run(
            [sys.executable, str(idup_script), "--split-world",
             "--output", str(tmpd / "idup.tsv"), "--prev-manifest", str(MANIFEST)],
            capture_output=True, text=True,
        )
        check("planned-id collision exits nonzero (fail-fast)",
              idup_proc.returncode != 0)
        check("planned-id collision names the duplicated id",
              "duplicate unit ids in manifest" in idup_proc.stderr
              and "mc.world.level.levelgen.synth" in idup_proc.stderr)

        # Fully-partitioned contract (Finding 4): a file dropped from a
        # fully-partitioned package's authored clusters must fail fast instead
        # of silently materializing a residual row. Drop DensityFunctions.java
        # from the noise cluster and require a nonzero exit naming the file.
        fp_src = ANALYZE.read_text(encoding="utf-8").replace(
            '            "DensityFunction.java", "DensityFunctions.java", "Heightmap.java",',
            '            "DensityFunction.java", "Heightmap.java",',
        ).replace(
            "REPO = Path(__file__).resolve().parent.parent",
            f"REPO = Path({str(REPO)!r})",
        )
        fp_script = tmpd / "analyze_graph_fp.py"
        fp_script.write_text(fp_src, encoding="utf-8")
        fp_proc = subprocess.run(
            [sys.executable, str(fp_script), "--split-world",
             "--output", str(tmpd / "fp.tsv"), "--prev-manifest", str(MANIFEST)],
            capture_output=True, text=True,
        )
        check("fully-partitioned dropped file exits nonzero (fail-fast)",
              fp_proc.returncode != 0)
        check("fully-partitioned dropped file names the unowned file",
              "fully partitioned but" in fp_proc.stderr
              and "DensityFunctions.java" in fp_proc.stderr)

        # Fully-partitioned durable-state guard (Finding 1): a flat (pre-split)
        # manifest whose levelgen row is in-progress must be rejected by
        # --split-world (carrying it would silently reset it). Build the flat
        # base once from the committed manifest (no flags), seed durable state
        # on the levelgen pre-split row, and require a nonzero exit.
        flat1 = tmpd / "flat1.tsv"
        run_analyze(out=flat1, prev=MANIFEST)
        fp_state = tmpd / "fp_state.tsv"
        fp_rows = list(csv.DictReader(flat1.open(newline=""), delimiter="\t"))
        for r in fp_rows:
            if r["id"] == "mc.world.level.levelgen":
                r["status"] = "translated"
                r["attempts"] = "3"
                r["notes"] = "in-flight triage"
        with fp_state.open("w", encoding="utf-8") as fh:
            fh.write("\t".join(fp_rows[0].keys()) + "\n")
            for r in fp_rows:
                fh.write("\t".join(r.values()) + "\n")
        fpg_proc = subprocess.run(
            [sys.executable, str(ANALYZE), "--split-world",
             "--output", str(tmpd / "fpg.tsv"), "--prev-manifest", str(fp_state)],
            capture_output=True, text=True,
        )
        check("fully-partitioned durable state exits nonzero (guard)",
              fpg_proc.returncode != 0
              and "would be silently lost" in fpg_proc.stderr)
        # And the negative control: a flat prev with only the retired authored
        # note (pending/0) splits cleanly — that is the committed state.
        flat_note = tmpd / "flat_note.tsv"
        fp_rows2 = list(csv.DictReader(flat1.open(newline=""), delimiter="\t"))
        for r in fp_rows2:
            if r["id"] == "mc.world.level.levelgen":
                r["notes"] = ("residual: the remaining ~40 independent feature leaves are "
                              "the #181 tail; they depend on feature.core (the hub's "
                              "reverse registration edges are generated content, not dep edges)")
        with flat_note.open("w", encoding="utf-8") as fh:
            fh.write("\t".join(fp_rows2[0].keys()) + "\n")
            for r in fp_rows2:
                fh.write("\t".join(r.values()) + "\n")
        fp_neg = subprocess.run(
            [sys.executable, str(ANALYZE), "--split-world",
             "--output", str(tmpd / "fpg_neg.tsv"), "--prev-manifest", str(flat_note)],
            capture_output=True, text=True,
        )
        check("fully-partitioned retired-note prev splits cleanly",
              fp_neg.returncode == 0)

        # Chunk is now fully partitioned too: a file dropped from its authored
        # clusters must fail fast instead of silently materializing a residual
        # row. Drop Strategy from the wire cluster and require a nonzero exit
        # naming the file.
        cfp_src = ANALYZE.read_text(encoding="utf-8").replace(
            '            "SingleValuePalette.java", "Strategy.java", "package-info.java",',
            '            "SingleValuePalette.java", "package-info.java",',
        ).replace(
            "REPO = Path(__file__).resolve().parent.parent",
            f"REPO = Path({str(REPO)!r})",
        )
        cfp_script = tmpd / "analyze_graph_cfp.py"
        cfp_script.write_text(cfp_src, encoding="utf-8")
        cfp_proc = subprocess.run(
            [sys.executable, str(cfp_script), "--split-world",
             "--output", str(tmpd / "cfp.tsv"), "--prev-manifest", str(MANIFEST)],
            capture_output=True, text=True,
        )
        check("chunk fully-partitioned dropped file exits nonzero (fail-fast)",
              cfp_proc.returncode != 0)
        check("chunk fully-partitioned dropped file names the unowned file",
              "fully partitioned but" in cfp_proc.stderr
              and "Strategy.java" in cfp_proc.stderr)

        # Chunk duplicate declaration: LevelChunkSection.java in both the wire
        # and access clusters must fail fast naming both owning units.
        cdup_src = ANALYZE.read_text(encoding="utf-8").replace(
            '            "PalettedContainerFactory.java", "PalettedContainerRO.java",\n'
            '            "SingleValuePalette.java", "Strategy.java", "package-info.java",',
            '            "PalettedContainerFactory.java", "PalettedContainerRO.java",\n'
            '            "SingleValuePalette.java", "Strategy.java", "package-info.java",\n'
            '            "LevelChunkSection.java",',
        ).replace(
            "REPO = Path(__file__).resolve().parent.parent",
            f"REPO = Path({str(REPO)!r})",
        )
        cdup_script = tmpd / "analyze_graph_cdup.py"
        cdup_script.write_text(cdup_src, encoding="utf-8")
        cdup_proc = subprocess.run(
            [sys.executable, str(cdup_script), "--split-world",
             "--output", str(tmpd / "cdup.tsv"), "--prev-manifest", str(MANIFEST)],
            capture_output=True, text=True,
        )
        check("chunk duplicate declaration exits nonzero (fail-fast)",
              cdup_proc.returncode != 0)
        check("chunk duplicate declaration names both owning units",
              "LevelChunkSection.java is declared in both "
              "mc.world.level.chunk.wire and mc.world.level.chunk.access"
              in cdup_proc.stderr)

        # Storage duplicate declaration: DataVersion.java in both the version
        # and leveldata clusters must fail fast naming both owning units.
        sdup_src = ANALYZE.read_text(encoding="utf-8").replace(
            '            "LevelData.java", "WritableLevelData.java", "ServerLevelData.java",',
            '            "LevelData.java", "DataVersion.java", "WritableLevelData.java",'
            ' "ServerLevelData.java",',
        ).replace(
            "REPO = Path(__file__).resolve().parent.parent",
            f"REPO = Path({str(REPO)!r})",
        )
        sdup_script = tmpd / "analyze_graph_sdup.py"
        sdup_script.write_text(sdup_src, encoding="utf-8")
        sdup_proc = subprocess.run(
            [sys.executable, str(sdup_script), "--split-world",
             "--output", str(tmpd / "sdup.tsv"), "--prev-manifest", str(MANIFEST)],
            capture_output=True, text=True,
        )
        check("storage duplicate declaration exits nonzero (fail-fast)",
              sdup_proc.returncode != 0)
        check("storage duplicate declaration names both owning units",
              "DataVersion.java is declared in both "
              "mc.world.level.storage.version and mc.world.level.storage.leveldata"
              in sdup_proc.stderr)

        # Storage residual carry (C-storage-residual): the residual keeps the
        # pre-split id, so seeded durable state on the flat mc.world.level.storage
        # row must survive onto the residual and every dependent must still
        # resolve its net.minecraft.world.level.storage token to it. Build the
        # flat manifest, seed state on the storage row, regen --split-world.
        flat2 = tmpd / "flat2.tsv"
        run_analyze(out=flat2, prev=MANIFEST)
        s_carry_state = tmpd / "s_carry_state.tsv"
        s_rows = list(csv.DictReader(flat2.open(newline=""), delimiter="\t"))
        for r in s_rows:
            if r["id"] == "mc.world.level.storage":
                r["status"] = "translated"
                r["attempts"] = "2"
                r["notes"] = "storage-wave note"
        with s_carry_state.open("w", encoding="utf-8") as fh:
            fh.write("\t".join(s_rows[0].keys()) + "\n")
            for r in s_rows:
                fh.write("\t".join(r.values()) + "\n")
        s_carry = tmpd / "s_carry.tsv"
        run_analyze("--split-world", out=s_carry, prev=s_carry_state)
        s_carry_rows = rows_of(s_carry)
        s_carry_by_id = {r["id"]: r for r in s_carry_rows}
        sr = s_carry_by_id["mc.world.level.storage"]
        check("storage residual carry: status/attempts/notes survive onto the residual",
              sr["status"] == "translated" and sr["attempts"] == "2"
              and "storage-wave note" in sr["notes"])
        # Every dependent still resolves its net.minecraft.world.level.storage
        # token to the residual (not the lowest-id cluster).
        s_carry_by_pkg: dict[str, list[dict]] = {}
        for r in s_carry_rows:
            s_carry_by_pkg.setdefault(r["java_package"], []).append(r)
        misresolved = []
        for r in s_carry_rows:
            for tok in (t.strip() for t in r["deps"].split(",") if t.strip()):
                if tok == "net.minecraft.world.level.storage":
                    if resolve_dep(tok, s_carry_by_id, s_carry_by_pkg)["id"] != "mc.world.level.storage":
                        misresolved.append(r["id"])
        check("storage residual carry: all dependents resolve storage token to the residual",
              not misresolved, misresolved[:5])

        # All-mode determinism (D-all-flags): two fresh runs of the canonical
        # --split-nbt --split-network --split-game --split-world --split-server
        # must be byte-identical (the committed-anchored `both` above only pins
        # one run against the artifact; this pins two independent regenerations).
        allf1 = tmpd / "allf1.tsv"
        allf2 = tmpd / "allf2.tsv"
        run_analyze("--split-nbt", "--split-network", "--split-game",
                    "--split-world", "--split-server", out=allf1, prev=MANIFEST)
        run_analyze("--split-nbt", "--split-network", "--split-game",
                    "--split-world", "--split-server", out=allf2, prev=allf1)
        check("all flags: two regenerations byte-identical",
              allf1.read_bytes() == allf2.read_bytes())

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
