#!/usr/bin/env python3
"""Write COVERAGE.tsv: one row per Java class of Paper 26.2, with its Rust home
and a method-level port-coverage estimate (ROADMAP.md section 0.1 / Batch 1A).

Run:  python3 scripts/coverage_ledger.py            # rewrite COVERAGE.tsv
      python3 scripts/coverage_ledger.py --check    # exit 1 if COVERAGE.tsv is stale
      python3 scripts/coverage_ledger.py --show net.minecraft.nbt.CompoundTag
      python3 scripts/coverage_ledger.py --summary  # status counts per crate

Deterministic and offline: reads the three Java roots of
scripts/analyze_graph.py (`working/Paper/...`), MANIFEST.tsv, and the
git-tracked `crates/*/src/**/*.rs`. No timestamps, no network. Runs in well
under two minutes.

Rows and columns
----------------
One row per `.java` file (= its top-level class) under the three roots,
`package-info.java` / `module-info.java` excluded, sorted by `java_path`.

  java_class       fully qualified name of the file's top-level class
  java_path        `root:relpath`, the MANIFEST.tsv `java_paths` identifier
  loc              newline count of the Java file (same as analyze_graph.py)
  bucket           ROADMAP 2.1 scope bucket:
                     A  minecraft root minus B and C (incl. DataConverter, Moonrise)
                     B  net.minecraft.data.**          (datagen)
                     C  net.minecraft.util.datafix.**  (DFU datafixers)
                     D  paper-server src/main minus org.bukkit.craftbukkit
                     E  org.bukkit.craftbukkit.**      (CraftBukkit)
                     F  paper-api
  hub              Appendix A hub list: `spine`, `adapter` (CraftBukkit hubs) or
                   `dfu-audit` (DataFixers); empty for non-hubs
  unit             owning MANIFEST.tsv unit id (via `java_paths`)
  current_crate    the owning unit's `crate` column
  assigned_crate   D15 home (see "Crate assignment" below)
  rust_path        mirrored Rust file (see "Rust home"), `-` if none
  other_paths      other same-named Rust files (misplaced or duplicate ports),
                   `;`-joined; a `x/mod.rs` that is the module of the class's
                   own package is not listed
  method_total     distinct method names declared in the Java file
  method_covered   how many of those names are covered (see "Method coverage")
  stub_count       `// STUB(...)` marker lines plus `todo!()`/`unimplemented!()`
                   macros in the Rust home
  rivettodo_count  `// RivetTodo(#N)` marker lines in the Rust home
  status           translation target (A, D, and B classes on the spine hub list):
                     absent    no mirrored Rust file
                     complete  every method covered, stub_count == 0 and
                               rivettodo_count == 0 (a RivetTodo is by definition
                               a partial port, scripts/check_markers.py)
                     partial   otherwise
                   other buckets: `n/a(datagen)` (B), `deferred(dfu-audit)` (C),
                   `adapter` (E, F); their Rust columns are `-`.

Rust home
---------
Rust files are the tracked `crates/*/src/**/*.rs`, minus test files (`tests.rs`,
`*_tests.rs`, `*_test.rs`, anything under `tests/`) and `generated/` trees. A
file `<dirs>/<stem>.rs` (or `<dirs>/<stem>/mod.rs`) is a candidate for a Java
class when `stem` equals the class name in snake case (compared with
underscores removed and lower-cased, so `AABB` -> `aabb.rs`). A candidate is a
*mirrored* path when either
  - exact mount: the file's crate mounts a Java package prefix at its `src/`
    root (MOUNTS) and `<dirs>` equals the rest of the class's package, or
  - suffix: `<dirs>` is non-empty and equals the trailing segments of the
    class's package (`rivet-world/src/levelgen/feature/` for
    `net.minecraft.world.level.levelgen.feature`).
Directory and package segments are compared split on `_` as well as `/` and
`.`, so `block_entity/` mirrors `...block.entity`. Among mirrored candidates
the exact mount wins, then the longest suffix, then the assigned crate, then
the current crate, then the lexicographically smallest path.
A class over 2,000 LOC may be split into a directory module (PORTING.md): when
the home is `x.rs` with a sibling `x/` directory, or `x/mod.rs`, every
non-test file under `x/` is part of the home, unless `x` is also a Java
sub-package of the class's package (then `x/` is that package, not a split).

Method coverage
---------------
Java side: `scripts/java_scan.py` (methods of the top-level and member types;
constructors, anonymous and local classes excluded; distinct by name, so
overloads count once). A Java method `m` is covered when the Rust home
(test code removed) has a `fn` whose name, with `r#` and underscores removed
and lower-cased, equals:
  - `m` lower-cased (`getBlockState` -> `get_block_state`);
  - for getters `getX...`, the name without `get` (`block_state`, PORTING.md),
    as a `fn` or as a `pub` struct field (packets and value types expose
    fields instead of getters);
  - for the Object/Comparable/Iterable trait methods, a std trait fn
    (OBJECT_ALIASES: `toString` -> `fmt`, `equals` -> `eq`, `hashCode` -> `hash`,
    `compareTo` -> `cmp`/`partial_cmp`, `iterator` -> `iter`/`into_iter`,
    `close` -> `drop`);
or, when `m` is overloaded in the Java file, a `fn` named `snake(m)_...`
(PORTING.md overload suffixes), or when a `// n/a: <reason>` comment in the
Rust home names `m` or `snake(m)` as a whole word.

Crate assignment (D15, ROADMAP 5.1)
-----------------------------------
Computed per Java package from the class graph, never chosen by hand. The
current crate is the package's majority MANIFEST.tsv `crate` (ties by name).
  1. `rivet-entity` content is destined for `rivet-server`. So is Paper server
     code: every D-bucket package, A-bucket packages the manifest maps to
     `rivet-api` (Paper patches in the minecraft root; paper-api is the only
     API source), and packages mapped to a crate that is not a workspace crate
     (`rivet-oracle` for net.minecraft.gametest).
  2. Live types are the classes of every `net.minecraft.*` package (bucket A)
     whose crate is `rivet-server` after step 1: world.entity, server,
     commands, ... (Moonrise/DataConverter are left out of the seed so Paper's
     mixin-style interfaces do not drag vanilla packages up).
  3. A package of another crate moves to `rivet-server`, as a whole package,
     when more than half of its classes directly reference a live type. This
     reproduces D15's examples without a hand list: block and item behaviour
     (world.level.block, world.item, block.entity), menus (world.inventory),
     effects and damage move; chunks, storage, levelgen, lighting, block-state
     tables and item value types (crafting, component) stay in rivet-world.
  4. Every other package keeps its current crate.
Class references are resolved imports (longest matching class prefix, so
nested and static imports land on the outer class), wildcard imports and
same-package classes matched by simple-name token, and inline fully
qualified names (ROADMAP Appendix B method).

Known blind spots
-----------------
- Coverage is name-based: a `fn` of the right name with a stub body counts as
  covered unless it carries a STUB marker or `todo!()`; a correctly ported
  method under a different name (beyond the rules above) counts as missing.
  Same-named methods of different nested types share one name.
- A class folded into its parent module (PORTING.md "<50 lines and
  package-private") or ported at a non-mirrored path shows `absent`; see
  `other_paths` for same-named files elsewhere.
- Rust `fn`s are collected from the whole home file, including fns of other
  types defined there.
- D15 step 3 is one hop with a majority threshold: a package that references
  a package moved in step 3 is not moved again (chasing references to a
  fixpoint collapses the 4,303-class SCC into one crate, which D15 rejects),
  and a minority of a package's classes may keep direct references to live
  types. Those references need the 1C dispatch/`LevelCtx` seams; they are
  not crate moves.
- Only the three Paper source roots are rows. Libraries Rivet ports from jars
  (DataFixerUpper's com.mojang.serialization/datafixers, most of Brigadier)
  have no rows; only the few classes Paper patches into the minecraft root do.
- The lexers (scripts/java_scan.py, scripts/rust_scan.py) are regex-based.
"""

import argparse
import csv
import io
import re
import sys
from collections import Counter, defaultdict
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import java_scan  # noqa: E402
import rust_scan  # noqa: E402
from analyze_graph import ROOTS, crate_for  # noqa: E402
from check_markers import RIVETTODO_RE, STUB_RE  # noqa: E402

REPO = Path(__file__).resolve().parent.parent
OUTPUT = REPO / "COVERAGE.tsv"
MANIFEST = REPO / "MANIFEST.tsv"

COLUMNS = [
    "java_class", "java_path", "loc", "bucket", "hub", "unit", "current_crate",
    "assigned_crate", "rust_path", "other_paths", "method_total", "method_covered",
    "stub_count", "rivettodo_count", "status",
]

TARGET_BUCKETS = {"A", "D"}
NON_TARGET_STATUS = {"B": "n/a(datagen)", "C": "deferred(dfu-audit)", "E": "adapter", "F": "adapter"}

# Crate -> Java package prefixes mounted at the crate's `src/` root.
MOUNTS = {
    "rivet-core": ["net.minecraft"],
    "rivet-util": ["net.minecraft.util"],
    "rivet-serialization": ["com.mojang.serialization"],
    "rivet-nbt": ["net.minecraft.nbt"],
    "rivet-text": ["net.minecraft.network.chat"],
    "rivet-brigadier": ["com.mojang.brigadier"],
    "rivet-registry": ["net.minecraft.core", "net.minecraft.resources", "net.minecraft.tags"],
    "rivet-protocol": ["net.minecraft.network"],
    "rivet-world": ["net.minecraft.world", "net.minecraft.world.level"],
    "rivet-entity": ["net.minecraft.world.entity"],
    "rivet-server": ["net.minecraft"],
}

OBJECT_ALIASES = {
    "toString": ["fmt", "to_string"],
    "equals": ["eq"],
    "hashCode": ["hash"],
    "compareTo": ["cmp", "partial_cmp"],
    "iterator": ["iter", "into_iter"],
    "close": ["drop"],
}

QUALIFIED_RE = re.compile(
    r"\b(?:net\.minecraft|ca\.spottedleaf|io\.papermc|com\.destroystokyo|org\.spigotmc|org\.bukkit)"
    r"(?:\.[A-Za-z_$][\w$]*)+"
)
IDENT_RE = re.compile(r"[A-Za-z_$][\w$]*")
NA_NOTE_RE = re.compile(r"^\s*//[/!]?\s*n/a:\s*(.+)$")
PUB_FIELD_RE = re.compile(r"\bpub(?:\([^)]*\))?\s+((?:r#)?[A-Za-z_][A-Za-z0-9_]*)\s*:(?!:)")

# ROADMAP.md Appendix A, resolved to source files (the two ambiguous simple
# names, Util and EntitySelector, resolved by the appendix's group).
HUBS = {
    # world.entity
    "minecraft:net/minecraft/world/entity/Entity.java": "spine",
    "minecraft:net/minecraft/world/entity/LivingEntity.java": "spine",
    "minecraft:net/minecraft/world/entity/Mob.java": "spine",
    "minecraft:net/minecraft/world/entity/EntityTypes.java": "spine",
    "minecraft:net/minecraft/world/entity/EntityType.java": "spine",
    "minecraft:net/minecraft/world/entity/ExperienceOrb.java": "spine",
    "minecraft:net/minecraft/world/entity/Leashable.java": "spine",
    "minecraft:net/minecraft/world/entity/AgeableMob.java": "spine",
    "minecraft:net/minecraft/world/entity/LightningBolt.java": "spine",
    "minecraft:net/minecraft/world/entity/EntityReference.java": "spine",
    "minecraft:net/minecraft/world/entity/PathfinderMob.java": "spine",
    "minecraft:net/minecraft/world/entity/ConversionParams.java": "spine",
    # world.entity.player
    "minecraft:net/minecraft/world/entity/player/Player.java": "spine",
    # world.entity, species and AI
    "minecraft:net/minecraft/world/entity/animal/equine/AbstractHorse.java": "spine",
    "minecraft:net/minecraft/world/entity/boss/enderdragon/EnderDragon.java": "spine",
    "minecraft:net/minecraft/world/entity/npc/villager/Villager.java": "spine",
    "minecraft:net/minecraft/world/entity/monster/piglin/PiglinAi.java": "spine",
    "minecraft:net/minecraft/world/entity/raid/Raid.java": "spine",
    "minecraft:net/minecraft/world/entity/vehicle/boat/AbstractBoat.java": "spine",
    "minecraft:net/minecraft/world/entity/projectile/arrow/AbstractArrow.java": "spine",
    "minecraft:net/minecraft/world/entity/vehicle/minecart/AbstractMinecart.java": "spine",
    "minecraft:net/minecraft/world/entity/animal/turtle/Turtle.java": "spine",
    "minecraft:net/minecraft/world/entity/monster/warden/Warden.java": "spine",
    "minecraft:net/minecraft/world/entity/monster/zombie/Zombie.java": "spine",
    "minecraft:net/minecraft/world/entity/monster/creaking/Creaking.java": "spine",
    "minecraft:net/minecraft/world/entity/item/ItemEntity.java": "spine",
    "minecraft:net/minecraft/world/entity/raid/Raider.java": "spine",
    "minecraft:net/minecraft/world/entity/projectile/Projectile.java": "spine",
    "minecraft:net/minecraft/world/entity/ai/Brain.java": "spine",
    "minecraft:net/minecraft/world/entity/ai/navigation/PathNavigation.java": "spine",
    "minecraft:net/minecraft/world/entity/ai/behavior/declarative/BehaviorBuilder.java": "spine",
    "minecraft:net/minecraft/world/entity/npc/villager/AbstractVillager.java": "spine",
    "minecraft:net/minecraft/world/entity/animal/golem/IronGolem.java": "spine",
    "minecraft:net/minecraft/world/entity/animal/Animal.java": "spine",
    "minecraft:net/minecraft/world/entity/monster/breeze/Breeze.java": "spine",
    "minecraft:net/minecraft/world/entity/projectile/ProjectileUtil.java": "spine",
    "minecraft:net/minecraft/world/entity/item/PrimedTnt.java": "spine",
    "minecraft:net/minecraft/world/entity/ai/behavior/BehaviorUtils.java": "spine",
    "minecraft:net/minecraft/world/entity/ai/memory/MemoryModuleType.java": "spine",
    "minecraft:net/minecraft/world/entity/monster/Monster.java": "spine",
    "minecraft:net/minecraft/world/entity/ai/goal/target/HurtByTargetGoal.java": "spine",
    "minecraft:net/minecraft/world/entity/ai/behavior/RandomStroll.java": "spine",
    "minecraft:net/minecraft/world/entity/boss/enderdragon/phases/EnderDragonPhase.java": "spine",
    "minecraft:net/minecraft/world/entity/boss/enderdragon/phases/AbstractDragonPhaseInstance.java": "spine",
    "minecraft:net/minecraft/world/entity/ai/sensing/SensorType.java": "spine",
    "minecraft:net/minecraft/world/entity/ai/goal/WaterAvoidingRandomStrollGoal.java": "spine",
    # server
    "minecraft:net/minecraft/server/MinecraftServer.java": "spine",
    "minecraft:net/minecraft/server/level/ServerPlayer.java": "spine",
    "minecraft:net/minecraft/server/level/ServerLevel.java": "spine",
    "minecraft:net/minecraft/server/players/PlayerList.java": "spine",
    "minecraft:net/minecraft/server/dedicated/DedicatedServer.java": "spine",
    "minecraft:net/minecraft/server/level/WorldGenRegion.java": "spine",
    "minecraft:net/minecraft/server/PlayerAdvancements.java": "spine",
    "minecraft:net/minecraft/server/level/GenerationChunkHolder.java": "spine",
    # world.level
    "minecraft:net/minecraft/world/level/Level.java": "spine",
    "minecraft:net/minecraft/world/level/NaturalSpawner.java": "spine",
    "minecraft:net/minecraft/world/level/BlockGetter.java": "spine",
    "minecraft:net/minecraft/world/level/ChunkPos.java": "spine",
    "minecraft:net/minecraft/world/level/LevelReader.java": "spine",
    "minecraft:net/minecraft/world/level/StructureManager.java": "spine",
    "minecraft:net/minecraft/world/level/LevelAccessor.java": "spine",
    "minecraft:net/minecraft/world/level/Explosion.java": "spine",
    "minecraft:net/minecraft/world/level/ScheduledTickAccess.java": "spine",
    "minecraft:net/minecraft/world/level/WorldGenLevel.java": "spine",
    "minecraft:net/minecraft/world/level/gamerules/GameRules.java": "spine",
    # world.level.block
    "minecraft:net/minecraft/world/level/block/Blocks.java": "spine",
    "minecraft:net/minecraft/world/level/block/state/BlockBehaviour.java": "spine",
    "minecraft:net/minecraft/world/level/block/Block.java": "spine",
    "minecraft:net/minecraft/world/level/block/entity/BeehiveBlockEntity.java": "spine",
    "minecraft:net/minecraft/world/level/block/entity/BlockEntity.java": "spine",
    "minecraft:net/minecraft/world/level/block/entity/ChestBlockEntity.java": "spine",
    "minecraft:net/minecraft/world/level/block/entity/BlockEntityTypes.java": "spine",
    "minecraft:net/minecraft/world/level/block/DispenserBlock.java": "spine",
    "minecraft:net/minecraft/world/level/block/state/properties/BlockStateProperties.java": "spine",
    "minecraft:net/minecraft/world/level/material/FluidState.java": "spine",
    "minecraft:net/minecraft/world/level/block/state/BlockState.java": "spine",
    "minecraft:net/minecraft/world/level/material/Fluids.java": "spine",
    # world.level.chunk
    "minecraft:net/minecraft/world/level/chunk/LevelChunk.java": "spine",
    "minecraft:net/minecraft/world/level/chunk/ChunkGenerator.java": "spine",
    "minecraft:net/minecraft/world/level/chunk/ChunkAccess.java": "spine",
    "minecraft:net/minecraft/world/level/chunk/ProtoChunk.java": "spine",
    "minecraft:net/minecraft/world/level/chunk/ImposterProtoChunk.java": "spine",
    "minecraft:net/minecraft/world/level/chunk/status/ChunkStep.java": "spine",
    # world.item and inventory
    "minecraft:net/minecraft/world/item/Items.java": "spine",
    "minecraft:net/minecraft/world/item/ItemStack.java": "spine",
    "minecraft:net/minecraft/world/inventory/AbstractContainerMenu.java": "spine",
    "minecraft:net/minecraft/world/item/Item.java": "spine",
    "minecraft:net/minecraft/world/item/enchantment/Enchantment.java": "spine",
    "minecraft:net/minecraft/world/item/enchantment/EnchantmentHelper.java": "spine",
    "minecraft:net/minecraft/core/component/DataComponents.java": "spine",
    "minecraft:net/minecraft/world/item/component/BlockItemStateProperties.java": "spine",
    "minecraft:net/minecraft/world/inventory/MenuType.java": "spine",
    "minecraft:net/minecraft/world/item/enchantment/effects/EnchantmentEntityEffect.java": "spine",
    "minecraft:net/minecraft/world/item/enchantment/providers/VanillaEnchantmentProviders.java": "spine",
    "minecraft:net/minecraft/world/item/enchantment/effects/EnchantmentValueEffect.java": "spine",
    # world.phys
    "minecraft:net/minecraft/world/phys/shapes/VoxelShape.java": "spine",
    "minecraft:net/minecraft/world/phys/AABB.java": "spine",
    "minecraft:net/minecraft/world/phys/Vec3.java": "spine",
    "minecraft:net/minecraft/world/phys/shapes/CollisionContext.java": "spine",
    # loot, damage, effects
    "minecraft:net/minecraft/world/level/storage/loot/LootTable.java": "spine",
    "minecraft:net/minecraft/world/damagesource/DamageSource.java": "spine",
    "minecraft:net/minecraft/world/level/storage/loot/LootContext.java": "spine",
    "minecraft:net/minecraft/world/level/storage/loot/entries/LootPoolSingletonContainer.java": "spine",
    "minecraft:net/minecraft/world/level/storage/loot/BuiltInLootTables.java": "spine",
    "minecraft:net/minecraft/world/effect/MobEffects.java": "spine",
    "minecraft:net/minecraft/world/level/storage/loot/functions/LootItemFunctions.java": "spine",
    "minecraft:net/minecraft/world/level/storage/loot/entries/LootPoolEntryContainer.java": "spine",
    # levelgen
    "minecraft:net/minecraft/world/level/levelgen/structure/templatesystem/StructureTemplate.java": "spine",
    "minecraft:net/minecraft/world/level/levelgen/structure/StructurePiece.java": "spine",
    "minecraft:net/minecraft/world/level/levelgen/Aquifer.java": "spine",
    "minecraft:net/minecraft/world/level/biome/Biome.java": "spine",
    "minecraft:net/minecraft/world/level/levelgen/blending/Blender.java": "spine",
    "minecraft:net/minecraft/world/level/levelgen/structure/Structure.java": "spine",
    "minecraft:net/minecraft/world/level/levelgen/feature/Feature.java": "spine",
    "minecraft:net/minecraft/world/level/levelgen/feature/foliageplacers/FoliagePlacer.java": "spine",
    "minecraft:net/minecraft/world/level/levelgen/Heightmap.java": "spine",
    "minecraft:net/minecraft/world/level/levelgen/structure/templatesystem/StructureTemplateManager.java": "spine",
    "minecraft:net/minecraft/world/level/levelgen/structure/pools/StructureTemplatePool.java": "spine",
    "minecraft:net/minecraft/world/level/levelgen/feature/configurations/TreeConfiguration.java": "spine",
    "minecraft:net/minecraft/world/level/levelgen/RandomState.java": "spine",
    "minecraft:net/minecraft/world/level/levelgen/structure/pools/StructurePoolElement.java": "spine",
    "minecraft:net/minecraft/data/worldgen/placement/PlacementUtils.java": "spine",
    "minecraft:net/minecraft/world/level/levelgen/structure/pieces/StructurePieceType.java": "spine",
    "minecraft:net/minecraft/world/level/levelgen/feature/treedecorators/TreeDecorator.java": "spine",
    "minecraft:net/minecraft/world/level/levelgen/placement/PlacedFeature.java": "spine",
    "minecraft:net/minecraft/world/level/levelgen/carver/CarvingContext.java": "spine",
    "minecraft:net/minecraft/world/level/levelgen/placement/PlacementContext.java": "spine",
    "minecraft:net/minecraft/world/level/levelgen/structure/StructureType.java": "spine",
    "minecraft:net/minecraft/data/worldgen/Pools.java": "spine",
    "minecraft:net/minecraft/data/worldgen/features/FeatureUtils.java": "spine",
    "minecraft:net/minecraft/world/level/levelgen/placement/PlacementModifierType.java": "spine",
    "minecraft:net/minecraft/world/level/levelgen/feature/treedecorators/TreeDecoratorType.java": "spine",
    "minecraft:net/minecraft/world/level/levelgen/placement/PlacementModifier.java": "spine",
    "minecraft:net/minecraft/world/level/levelgen/structure/pieces/StructurePieceSerializationContext.java": "spine",
    # storage
    "minecraft:net/minecraft/world/level/saveddata/maps/MapItemSavedData.java": "spine",
    "minecraft:net/minecraft/world/level/storage/LevelStorageSource.java": "spine",
    "minecraft:net/minecraft/world/level/storage/ValueInput.java": "spine",
    # core, util, resources
    "minecraft:net/minecraft/util/Util.java": "spine",
    "minecraft:net/minecraft/core/BlockPos.java": "spine",
    "minecraft:net/minecraft/util/Mth.java": "spine",
    "minecraft:net/minecraft/core/Direction.java": "spine",
    "minecraft:net/minecraft/core/registries/BuiltInRegistries.java": "spine",
    "minecraft:net/minecraft/core/registries/Registries.java": "spine",
    "minecraft:net/minecraft/resources/RegistryDataLoader.java": "spine",
    "minecraft:net/minecraft/core/SectionPos.java": "spine",
    "minecraft:net/minecraft/resources/Identifier.java": "spine",
    "minecraft:net/minecraft/core/Holder.java": "spine",
    "minecraft:net/minecraft/core/Registry.java": "spine",
    "minecraft:net/minecraft/util/RandomSource.java": "spine",
    "minecraft:net/minecraft/resources/ResourceKey.java": "spine",
    # network
    "minecraft:net/minecraft/network/FriendlyByteBuf.java": "spine",
    "minecraft:net/minecraft/network/codec/ByteBufCodecs.java": "spine",
    "minecraft:net/minecraft/nbt/CompoundTag.java": "spine",
    "minecraft:net/minecraft/network/chat/ComponentSerialization.java": "spine",
    "minecraft:net/minecraft/network/protocol/game/ClientGamePacketListener.java": "spine",
    "minecraft:net/minecraft/network/chat/Component.java": "spine",
    "minecraft:net/minecraft/network/syncher/EntityDataSerializers.java": "spine",
    "minecraft:net/minecraft/network/protocol/game/GamePacketTypes.java": "spine",
    "minecraft:net/minecraft/network/protocol/game/ClientboundAddEntityPacket.java": "spine",
    "minecraft:net/minecraft/network/protocol/game/ServerGamePacketListener.java": "spine",
    "minecraft:net/minecraft/network/protocol/Packet.java": "spine",
    "minecraft:net/minecraft/network/PacketListener.java": "spine",
    "minecraft:net/minecraft/network/protocol/common/CommonPacketTypes.java": "spine",
    "minecraft:net/minecraft/network/RegistryFriendlyByteBuf.java": "spine",
    "minecraft:net/minecraft/network/protocol/common/ClientCommonPacketListener.java": "spine",
    "minecraft:net/minecraft/network/protocol/configuration/ConfigurationPacketTypes.java": "spine",
    "minecraft:net/minecraft/network/protocol/configuration/ClientConfigurationPacketListener.java": "spine",
    # commands, advancements, sounds, gametest
    "minecraft:net/minecraft/sounds/SoundEvents.java": "spine",
    "minecraft:net/minecraft/commands/Commands.java": "spine",
    "minecraft:net/minecraft/commands/CommandSourceStack.java": "spine",
    "minecraft:net/minecraft/commands/arguments/selector/EntitySelectorParser.java": "spine",
    "minecraft:net/minecraft/gametest/framework/GameTestInfo.java": "spine",
    "minecraft:net/minecraft/commands/arguments/selector/EntitySelector.java": "spine",
    "minecraft:net/minecraft/advancements/predicates/entity/EntityPredicate.java": "spine",
    "minecraft:net/minecraft/commands/arguments/EntityArgument.java": "spine",
    "minecraft:net/minecraft/commands/synchronization/ArgumentUtils.java": "spine",
    "minecraft:net/minecraft/gametest/framework/GameTestInstance.java": "spine",
    "minecraft:net/minecraft/advancements/triggers/CriteriaTriggers.java": "spine",
    "minecraft:net/minecraft/sounds/SoundEvent.java": "spine",
    # Moonrise and Paper
    "minecraft:ca/spottedleaf/moonrise/patches/chunk_system/scheduling/NewChunkHolder.java": "spine",
    "minecraft:ca/spottedleaf/moonrise/patches/chunk_system/scheduling/ChunkTaskScheduler.java": "spine",
    "paper-server:io/papermc/paper/util/MCUtil.java": "spine",
    "paper-server:ca/spottedleaf/moonrise/common/PlatformHooks.java": "spine",
    "paper-server:io/papermc/paper/registry/data/util/Conversions.java": "spine",
    "paper-server:io/papermc/paper/registry/set/PaperRegistrySets.java": "spine",
    "minecraft:ca/spottedleaf/moonrise/patches/chunk_system/level/ChunkSystemServerLevel.java": "spine",
    "minecraft:ca/spottedleaf/moonrise/patches/chunk_system/level/chunk/ChunkData.java": "spine",
    # Adapter track, not P1
    "paper-server:org/bukkit/craftbukkit/entity/CraftPlayer.java": "adapter",
    "paper-server:org/bukkit/craftbukkit/CraftServer.java": "adapter",
    "paper-server:org/bukkit/craftbukkit/inventory/CraftMetaItem.java": "adapter",
    "paper-server:org/bukkit/craftbukkit/CraftWorld.java": "adapter",
    "paper-server:org/bukkit/craftbukkit/entity/CraftEntity.java": "adapter",
    "paper-server:org/bukkit/craftbukkit/entity/CraftHumanEntity.java": "adapter",
    "paper-server:org/bukkit/craftbukkit/inventory/CraftItemStack.java": "adapter",
    "paper-server:org/bukkit/craftbukkit/block/CraftBlock.java": "adapter",
    "paper-server:org/bukkit/craftbukkit/util/CraftMagicNumbers.java": "adapter",
    "paper-server:org/bukkit/craftbukkit/inventory/CraftInventory.java": "adapter",
    "paper-server:org/bukkit/craftbukkit/CraftRegistry.java": "adapter",
    "paper-server:org/bukkit/craftbukkit/inventory/SerializableMeta.java": "adapter",
    # DFU audit, not P1
    "minecraft:net/minecraft/util/datafix/DataFixers.java": "dfu-audit",
}


def bucket_for(root: str, pkg: str) -> str:
    if root == "paper-api":
        return "F"
    if root == "paper-server":
        return "E" if pkg == "org.bukkit.craftbukkit" or pkg.startswith("org.bukkit.craftbukkit.") else "D"
    if pkg == "net.minecraft.data" or pkg.startswith("net.minecraft.data."):
        return "B"
    if pkg == "net.minecraft.util.datafix" or pkg.startswith("net.minecraft.util.datafix."):
        return "C"
    return "A"


def segs(dotted_or_path: str) -> list[str]:
    """Split a package / dir path on `.`, `/` and `_`, lower-cased."""
    return [s for s in re.split(r"[./_]", dotted_or_path.lower()) if s]


def norm(name: str) -> str:
    return name.removeprefix("r#").replace("_", "").lower()


class JavaClass:
    __slots__ = ("root", "rel", "path", "pkg", "name", "fqn", "loc", "bucket", "text", "code")

    def __init__(self, root: str, base: Path, f: Path):
        self.root = root
        self.rel = f.relative_to(base).as_posix()
        self.path = f"{root}:{self.rel}"
        self.text = f.read_text(encoding="utf-8", errors="replace")
        self.pkg = java_scan.java_package(self.text)
        self.name = f.stem
        self.fqn = f"{self.pkg}.{self.name}" if self.pkg else self.name
        self.loc = self.text.count("\n")
        self.bucket = bucket_for(root, self.pkg)
        self.code = ""


def load_java() -> list[JavaClass]:
    classes = []
    for root, base in ROOTS.items():
        if not base.is_dir():
            sys.exit(f"missing source root: {base} (run Paper's gradle setup first)")
        for f in base.rglob("*.java"):
            if f.name in ("package-info.java", "module-info.java"):
                continue
            classes.append(JavaClass(root, base, f))
    classes.sort(key=lambda c: c.path)
    for c in classes:
        c.code = java_scan.clean_java(c.text)
    return classes


def load_manifest_units() -> dict[str, tuple[str, str]]:
    """java_path -> (unit id, crate)."""
    owners: dict[str, tuple[str, str]] = {}
    with MANIFEST.open(newline="", encoding="utf-8") as fh:
        for row in csv.DictReader(fh, delimiter="\t"):
            for p in filter(None, row["java_paths"].split(",")):
                owners[p] = (row["id"], row["crate"])
    return owners


# ---------------------------------------------------------------- class graph

def class_references(classes: list[JavaClass]) -> dict[str, set[str]]:
    """fqn -> set of referenced fqns (outer classes only, self excluded)."""
    by_fqn = {c.fqn: c for c in classes}
    by_pkg: dict[str, dict[str, str]] = defaultdict(dict)
    for c in classes:
        by_pkg[c.pkg][c.name] = c.fqn

    def resolve(dotted: str) -> str | None:
        parts = dotted.split(".")
        for n in range(len(parts), 1, -1):
            cand = ".".join(parts[:n])
            if cand in by_fqn:
                return cand
        return None

    refs: dict[str, set[str]] = {}
    for c in classes:
        out: set[str] = set()
        tokens = set(IDENT_RE.findall(c.code))
        for imp in java_scan.java_imports(c.text):
            if imp.endswith(".*"):
                base = imp[:-2]
                if base in by_pkg:
                    out.update(fq for simple, fq in by_pkg[base].items() if simple in tokens)
                else:
                    owner = resolve(base)
                    if owner:
                        out.add(owner)
            else:
                owner = resolve(imp)
                if owner:
                    out.add(owner)
        out.update(fq for simple, fq in by_pkg[c.pkg].items() if simple in tokens)
        for q in QUALIFIED_RE.findall(c.code):
            owner = resolve(q)
            if owner:
                out.add(owner)
        out.discard(c.fqn)
        refs[c.fqn] = out
    return refs


def assign_crates(classes: list[JavaClass], current: dict[str, str],
                  refs: dict[str, set[str]]) -> dict[str, str]:
    """package -> D15 crate, for every package holding A, B or D classes."""
    workspace = {p.name for p in (REPO / "crates").iterdir() if p.is_dir()}
    pkg_classes: dict[str, list[JavaClass]] = defaultdict(list)
    for c in classes:
        if c.bucket in TARGET_BUCKETS or c.bucket == "B":
            pkg_classes[c.pkg].append(c)
    pkg_crate: dict[str, str] = {}
    for pkg, cs in pkg_classes.items():
        counts = Counter(current[c.path] for c in cs)
        crate = sorted(counts.items(), key=lambda kv: (-kv[1], kv[0]))[0][0]
        if cs[0].bucket == "D" or crate in ("rivet-entity", "rivet-api") or crate not in workspace:
            crate = "rivet-server"
        pkg_crate[pkg] = crate
    live = {
        c.fqn for c in classes
        if c.bucket == "A" and c.pkg.startswith("net.minecraft.")
        and pkg_crate[c.pkg] == "rivet-server"
    }
    moved = {}
    for pkg, cs in pkg_classes.items():
        if pkg_crate[pkg] == "rivet-server":
            continue
        if 2 * sum(1 for c in cs if refs[c.fqn] & live) > len(cs):
            moved[pkg] = "rivet-server"
    pkg_crate.update(moved)
    return pkg_crate


# ---------------------------------------------------------------- rust side

class RustFile:
    __slots__ = ("rel", "crate", "dirs", "stem", "key", "text", "code")

    def __init__(self, rel: Path):
        self.rel = rel.as_posix()
        parts = rel.parts  # crates/<crate>/src/...
        self.crate = parts[1]
        inner = list(parts[3:])
        if inner[-1] == "mod.rs":
            self.stem = inner[-2]
            self.dirs = inner[:-2]
        else:
            self.stem = inner[-1][:-3]
            self.dirs = inner[:-1]
        self.key = norm(self.stem)
        self.text: str | None = None
        self.code: str | None = None

    def load(self) -> None:
        if self.text is None:
            self.text = (REPO / self.rel).read_text(encoding="utf-8", errors="replace")
            self.code = rust_scan.strip_test_code(rust_scan.blank_comments_and_strings(self.text))


def load_rust() -> list[RustFile]:
    files = []
    for rel in rust_scan.tracked_crate_sources():
        inner = rel.parts[3:]
        if not inner or rust_scan.is_test_file(Path(*inner)) or rust_scan.is_generated_path(rel):
            continue
        if inner == ("lib.rs",) or inner == ("main.rs",) or inner[-1] in ("lib.rs", "main.rs"):
            continue
        files.append(RustFile(rel))
    return files


def mirror_score(rf: RustFile, pkg: str) -> tuple[int, int] | None:
    """(exact mount?, matched dir segments) if rf sits at a mirrored path."""
    dirs = segs("/".join(rf.dirs))
    psegs = segs(pkg)
    for prefix in MOUNTS.get(rf.crate, []):
        if pkg == prefix or pkg.startswith(prefix + "."):
            if dirs == segs(pkg[len(prefix):]):
                return (1, len(dirs))
    if dirs and len(dirs) <= len(psegs) and psegs[-len(dirs):] == dirs:
        return (0, len(dirs))
    return None


def is_package_module(rf: RustFile, pkg: str) -> bool:
    """`x/mod.rs` whose module path mirrors the class's own package (the
    package's module file, not a port of the class)."""
    if not rf.rel.endswith("/mod.rs"):
        return False
    mod = segs("/".join(rf.dirs + [rf.stem]))
    psegs = segs(pkg)
    return len(mod) <= len(psegs) and psegs[-len(mod):] == mod


def home_files(home: RustFile, pkg: str, by_path: dict[str, RustFile],
               java_pkgs: set[str]) -> list[RustFile]:
    """The home file plus its directory-module files (PORTING.md split)."""
    files = [home]
    if f"{pkg}.{home.stem.replace('_', '')}" in java_pkgs or f"{pkg}.{home.stem}" in java_pkgs:
        return files
    if home.rel.endswith("/mod.rs"):
        mod_dir = home.rel[: -len("mod.rs")]
    else:
        mod_dir = home.rel[: -len(".rs")] + "/"
    for path in sorted(by_path):
        if path.startswith(mod_dir) and path != home.rel:
            files.append(by_path[path])
    return files


def method_coverage(java_methods: list[str], files: list[RustFile]) -> tuple[int, int, list[str]]:
    names = Counter(java_methods)
    rust_fns: set[str] = set()
    raw_fns: set[str] = set()
    fields: set[str] = set()
    na_text: list[str] = []
    for rf in files:
        rf.load()
        for fn in rust_scan.fn_names(rf.code):
            raw_fns.add(fn.removeprefix("r#"))
            rust_fns.add(norm(fn))
        fields.update(norm(f) for f in PUB_FIELD_RE.findall(rf.code))
        for line in rf.text.splitlines():
            m = NA_NOTE_RE.match(line)
            if m:
                na_text.append(m.group(1))
    na_blob = "\n".join(na_text)
    missing = []
    covered = 0
    for m in sorted(names):
        snake = java_scan.snake_case(m)
        ok = norm(m) in rust_fns
        if not ok and re.match(r"get[A-Z]", m):
            ok = norm(m[3:]) in rust_fns or norm(m[3:]) in fields
        if not ok and m in OBJECT_ALIASES:
            ok = any(a in raw_fns for a in OBJECT_ALIASES[m])
        if not ok and names[m] > 1:
            ok = any(f.startswith(snake + "_") for f in raw_fns)
        if not ok and na_blob:
            ok = re.search(rf"\b(?:{re.escape(m)}|{re.escape(snake)})\b", na_blob) is not None
        if ok:
            covered += 1
        else:
            missing.append(m)
    return len(names), covered, missing


def marker_counts(files: list[RustFile]) -> tuple[int, int]:
    stubs = todos = 0
    for rf in files:
        rf.load()
        for line in rf.text.splitlines():
            if STUB_RE.match(line):
                stubs += 1
            elif RIVETTODO_RE.match(line):
                todos += 1
        stubs += len(rust_scan.TODO_MACRO_RE.findall(rf.code))
    return stubs, todos


# ---------------------------------------------------------------- ledger

def print_detail(c: JavaClass, row: dict[str, str], missing: list[str] | None) -> None:
    print(f"{c.fqn}  ({c.path}, {c.loc} LOC)")
    for k in COLUMNS[3:]:
        print(f"  {k}: {row[k]}")
    if missing is not None:
        print(f"  missing methods ({len(missing)}): {' '.join(missing)}")


def build_rows(detail: str | None = None) -> list[dict[str, str]]:
    classes = load_java()
    owners = load_manifest_units()
    current = {}
    unit_of = {}
    for c in classes:
        unit, crate = owners.get(c.path, ("", crate_for(c.pkg)))
        unit_of[c.path] = unit
        current[c.path] = crate
    refs = class_references(classes)
    pkg_crate = assign_crates(classes, current, refs)
    java_pkgs = {c.pkg for c in classes}

    rust = load_rust()
    by_key: dict[str, list[RustFile]] = defaultdict(list)
    for rf in rust:
        by_key[rf.key].append(rf)
    by_path = {rf.rel: rf for rf in rust}

    rows = []
    for c in classes:
        hub = HUBS.get(c.path, "")
        target = c.bucket in TARGET_BUCKETS or (c.bucket == "B" and hub == "spine")
        row = {
            "java_class": c.fqn, "java_path": c.path, "loc": str(c.loc), "bucket": c.bucket,
            "hub": hub, "unit": unit_of[c.path], "current_crate": current[c.path],
        }
        if not target:
            row.update({k: "-" for k in COLUMNS if k not in row})
            row["status"] = NON_TARGET_STATUS[c.bucket]
            if detail is not None and detail in (c.fqn, c.path):
                print_detail(c, row, None)
            rows.append(row)
            continue
        assigned = pkg_crate[c.pkg]
        cands = by_key.get(c.name.lower(), [])
        best = None
        best_key = None
        for rf in cands:
            score = mirror_score(rf, c.pkg)
            if score is None:
                continue
            key = (score[0], score[1], rf.crate == assigned, rf.crate == current[c.path])
            if best is None or key > best_key or (key == best_key and rf.rel < best.rel):
                best, best_key = rf, key
        others = sorted(rf.rel for rf in cands if rf is not best and not is_package_module(rf, c.pkg))
        methods = [n for _, n in java_scan.scan_methods(c.code)]
        if best is None:
            total = len(set(methods))
            row.update({
                "assigned_crate": assigned, "rust_path": "-", "other_paths": ";".join(others),
                "method_total": str(total), "method_covered": "0", "stub_count": "0",
                "rivettodo_count": "0", "status": "absent",
            })
            missing = sorted(set(methods))
        else:
            files = home_files(best, c.pkg, by_path, java_pkgs)
            total, covered, missing = method_coverage(methods, files)
            stubs, todos = marker_counts(files)
            complete = covered == total and stubs == 0 and todos == 0
            row.update({
                "assigned_crate": assigned, "rust_path": best.rel, "other_paths": ";".join(others),
                "method_total": str(total), "method_covered": str(covered),
                "stub_count": str(stubs), "rivettodo_count": str(todos),
                "status": "complete" if complete else "partial",
            })
        if detail is not None and detail in (c.fqn, c.path):
            print_detail(c, row, missing)
        rows.append(row)
    return rows


def render(rows: list[dict[str, str]]) -> str:
    buf = io.StringIO()
    buf.write("\t".join(COLUMNS) + "\n")
    for r in rows:
        buf.write("\t".join(r[k] for k in COLUMNS) + "\n")
    return buf.getvalue()


def summarize(rows: list[dict[str, str]]) -> str:
    out = []
    by: dict[str, Counter] = defaultdict(Counter)
    loc_by: dict[str, Counter] = defaultdict(Counter)
    for r in rows:
        if r["status"] not in ("absent", "partial", "complete"):
            by["(non-target)"][r["status"]] += 1
            loc_by["(non-target)"][r["status"]] += int(r["loc"])
            continue
        for key in ("TOTAL", r["assigned_crate"]):
            by[key][r["status"]] += 1
            loc_by[key][r["status"]] += int(r["loc"])
        if r["hub"] == "spine":
            by["(spine hubs)"][r["status"]] += 1
            loc_by["(spine hubs)"][r["status"]] += int(r["loc"])
    out.append("scope\tabsent\tpartial\tcomplete\tabsent_loc\tpartial_loc\tcomplete_loc")
    for key in sorted(by, key=lambda k: (k.startswith("("), k == "TOTAL", k)):
        if key == "(non-target)":
            continue
        cnt, loc = by[key], loc_by[key]
        out.append("\t".join([key] + [str(cnt[s]) for s in ("absent", "partial", "complete")]
                             + [str(loc[s]) for s in ("absent", "partial", "complete")]))
    nt = by.get("(non-target)")
    if nt:
        out.append("non-target: " + ", ".join(
            f"{s} {nt[s]} classes / {loc_by['(non-target)'][s]} LOC" for s in sorted(nt)))
    return "\n".join(out)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--check", action="store_true", help="exit 1 if COVERAGE.tsv is not current")
    ap.add_argument("--show", metavar="CLASS", help="print one class's row and missing methods")
    ap.add_argument("--summary", action="store_true", help="print status counts per crate")
    ap.add_argument("--output", type=Path, default=OUTPUT)
    args = ap.parse_args()

    rows = build_rows(args.show)
    text = render(rows)
    if args.summary:
        print(summarize(rows))
    if args.show:
        return 0
    if args.check:
        current = args.output.read_text(encoding="utf-8") if args.output.exists() else ""
        if current != text:
            print(f"{args.output.name} is stale: run python3 scripts/coverage_ledger.py", file=sys.stderr)
            return 1
        print(f"{args.output.name} is current ({len(rows)} rows)")
        return 0
    args.output.write_text(text, encoding="utf-8")
    print(f"wrote {args.output.name} ({len(rows)} rows)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
