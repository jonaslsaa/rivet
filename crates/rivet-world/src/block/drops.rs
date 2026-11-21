//! Paper 26.2 block-breaking drop and experience resolution (issue #532).
//!
//! Mirrors the server-authoritative drop path in:
//! - `net.minecraft.server.level.ServerPlayerGameMode#destroyBlock`
//! - `net.minecraft.world.entity.player.Player#preventsBlockDrops`
//! - `net.minecraft.world.entity.player.Player#hasCorrectToolForDrops`
//! - `net.minecraft.world.item.component.Tool` / `ToolMaterial` / `ShearsItem`
//! - `net.minecraft.world.level.block.Block#getDrops` / `dropResources` / `popResource` / `popExperience` / `getExpDrop`
//! - `net.minecraft.world.level.block.DropExperienceBlock` / `RedStoneOreBlock` / `SpawnerBlock`
//! - `net.minecraft.world.item.enchantment.EnchantmentHelper#processBlockExperience`
//! - `net.minecraft.world.level.storage.loot.LootTable` and block loot conditions/functions
//!   (`survives_explosion`, `block_state_property`, `match_tool`, `any_of`, `all_of`, `inverted`,
//!   `table_bonus`, `random_chance`, `set_count`, `apply_bonus`, `limit_count`, `explosion_decay`).

use indexmap::IndexMap;
use rivet_registry::block_state::BlockState;
use rivet_registry::block_state_property::PropertyValue;
use rivet_registry::core::{BlockPos, GameType};
use rivet_registry::state_definition::StateDefinition;
use rivet_util::mth;
use rivet_util::random::RandomSource;
use serde_json::Value;
use thiserror::Error;

/// Tool material tiers matching Paper `net.minecraft.world.item.ToolMaterial`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ToolTier {
    Wood,
    Stone,
    Copper,
    Iron,
    Diamond,
    Gold,
    Netherite,
}

impl ToolTier {
    /// Block tag of blocks this tier cannot harvest for drops
    /// (`ToolMaterial.incorrectBlocksForDrops`).
    #[inline]
    pub const fn incorrect_blocks_for_drops_tag(self) -> &'static str {
        match self {
            Self::Wood => "minecraft:incorrect_for_wooden_tool",
            Self::Stone => "minecraft:incorrect_for_stone_tool",
            Self::Copper => "minecraft:incorrect_for_copper_tool",
            Self::Iron => "minecraft:incorrect_for_iron_tool",
            Self::Diamond => "minecraft:incorrect_for_diamond_tool",
            Self::Gold => "minecraft:incorrect_for_gold_tool",
            Self::Netherite => "minecraft:incorrect_for_netherite_tool",
        }
    }

    /// Base mining speed of this tier (`ToolMaterial.speed`).
    #[inline]
    pub const fn speed(self) -> f32 {
        match self {
            Self::Wood => 2.0,
            Self::Stone => 4.0,
            Self::Copper => 5.0,
            Self::Iron => 6.0,
            Self::Diamond => 8.0,
            Self::Gold => 12.0,
            Self::Netherite => 9.0,
        }
    }

    /// Item ID prefix for tools of this tier.
    #[inline]
    pub const fn item_prefix(self) -> &'static str {
        match self {
            Self::Wood => "wooden",
            Self::Stone => "stone",
            Self::Copper => "copper",
            Self::Iron => "iron",
            Self::Diamond => "diamond",
            Self::Gold => "golden",
            Self::Netherite => "netherite",
        }
    }
}

/// Standard harvest tool categories (`ToolMaterial.applyToolProperties`).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ToolKind {
    Pickaxe,
    Axe,
    Shovel,
    Hoe,
}

impl ToolKind {
    /// Block tag mined efficiently by this tool category.
    #[inline]
    pub const fn mineable_tag(self) -> &'static str {
        match self {
            Self::Pickaxe => "minecraft:mineable/pickaxe",
            Self::Axe => "minecraft:mineable/axe",
            Self::Shovel => "minecraft:mineable/shovel",
            Self::Hoe => "minecraft:mineable/hoe",
        }
    }

    /// Item ID suffix for this tool category.
    #[inline]
    pub const fn item_suffix(self) -> &'static str {
        match self {
            Self::Pickaxe => "pickaxe",
            Self::Axe => "axe",
            Self::Shovel => "shovel",
            Self::Hoe => "hoe",
        }
    }
}

/// Block set matcher inside a [`ToolRule`].
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ToolRuleMatcher {
    /// Matches any block in the given block tag (e.g. `"minecraft:mineable/pickaxe"`).
    Tag(&'static str),
    /// Matches a single block by namespaced ID (e.g. `"minecraft:cobweb"`).
    Block(&'static str),
}

impl ToolRuleMatcher {
    #[inline]
    pub fn matches(self, state: BlockState) -> bool {
        match self {
            Self::Tag(tag) => state.is_in_tag(tag),
            Self::Block(id) => block_name_matches(state.block().name(), id),
        }
    }
}

/// Single rule inside Paper's `net.minecraft.world.item.component.Tool.Rule`.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ToolRule {
    pub blocks: ToolRuleMatcher,
    pub speed: Option<f32>,
    pub correct_for_drops: Option<bool>,
}

impl ToolRule {
    /// `Tool.Rule.minesAndDrops(blocks, speed)`.
    #[inline]
    pub const fn mines_and_drops(blocks: ToolRuleMatcher, speed: f32) -> Self {
        Self {
            blocks,
            speed: Some(speed),
            correct_for_drops: Some(true),
        }
    }

    /// `Tool.Rule.deniesDrops(blocks)`.
    #[inline]
    pub const fn denies_drops(blocks: ToolRuleMatcher) -> Self {
        Self {
            blocks,
            speed: None,
            correct_for_drops: Some(false),
        }
    }

    /// `Tool.Rule.overrideSpeed(blocks, speed)`.
    #[inline]
    pub const fn override_speed(blocks: ToolRuleMatcher, speed: f32) -> Self {
        Self {
            blocks,
            speed: Some(speed),
            correct_for_drops: None,
        }
    }
}

/// Paper's `net.minecraft.world.item.component.Tool` data component.
///
/// Rule order is observable: `is_correct_for_drops` and `get_mining_speed`
/// iterate `rules` in order and return the first matching rule that specifies
/// the queried property.
#[derive(Clone, PartialEq, Debug)]
pub struct ToolSpec {
    pub rules: Vec<ToolRule>,
    pub default_mining_speed: f32,
    pub damage_per_block: i32,
    pub can_destroy_blocks_in_creative: bool,
}

impl ToolSpec {
    /// Paper `ToolMaterial.applyToolProperties(properties, minesEfficiently, ...)`.
    pub fn for_tool(tier: ToolTier, kind: ToolKind) -> Self {
        Self {
            rules: vec![
                ToolRule::denies_drops(ToolRuleMatcher::Tag(tier.incorrect_blocks_for_drops_tag())),
                ToolRule::mines_and_drops(ToolRuleMatcher::Tag(kind.mineable_tag()), tier.speed()),
            ],
            default_mining_speed: 1.0,
            damage_per_block: 1,
            can_destroy_blocks_in_creative: true,
        }
    }

    /// Paper `ShearsItem.createToolProperties()`.
    pub fn shears() -> Self {
        Self {
            rules: vec![
                ToolRule::mines_and_drops(ToolRuleMatcher::Block("minecraft:cobweb"), 15.0),
                ToolRule::override_speed(
                    ToolRuleMatcher::Tag("minecraft:shears_extreme_breaking_speed"),
                    15.0,
                ),
                ToolRule::override_speed(
                    ToolRuleMatcher::Tag("minecraft:shears_major_breaking_speed"),
                    5.0,
                ),
                ToolRule::override_speed(
                    ToolRuleMatcher::Tag("minecraft:shears_minor_breaking_speed"),
                    2.0,
                ),
            ],
            default_mining_speed: 1.0,
            damage_per_block: 1,
            can_destroy_blocks_in_creative: true,
        }
    }

    /// Paper `ToolMaterial.applySwordProperties(...)`.
    pub fn sword() -> Self {
        Self {
            rules: vec![
                ToolRule::mines_and_drops(ToolRuleMatcher::Block("minecraft:cobweb"), 15.0),
                ToolRule::override_speed(
                    ToolRuleMatcher::Tag("minecraft:sword_instantly_mines"),
                    f32::MAX,
                ),
                ToolRule::override_speed(ToolRuleMatcher::Tag("minecraft:sword_efficient"), 1.5),
            ],
            default_mining_speed: 1.0,
            damage_per_block: 2,
            can_destroy_blocks_in_creative: false,
        }
    }

    /// Paper `Tool.isCorrectForDrops(BlockState)`.
    pub fn is_correct_for_drops(&self, state: BlockState) -> bool {
        for rule in &self.rules {
            if let Some(correct) = rule.correct_for_drops
                && rule.blocks.matches(state)
            {
                return correct;
            }
        }
        false
    }

    /// Paper `Tool.getMiningSpeed(BlockState)`.
    pub fn get_mining_speed(&self, state: BlockState) -> f32 {
        for rule in &self.rules {
            if let Some(speed) = rule.speed
                && rule.blocks.matches(state)
            {
                return speed;
            }
        }
        self.default_mining_speed
    }
}

/// Held tool snapshot passed to drop resolution (Paper `LootContextParams.TOOL`).
#[derive(Clone, PartialEq, Debug, Default)]
pub struct ToolStack {
    /// Namespaced item ID (e.g. `"minecraft:diamond_pickaxe"`), or `None` for empty hand.
    pub item_id: Option<String>,
    /// Paper `DataComponents.TOOL` component if this item has tool rules.
    pub tool: Option<ToolSpec>,
    /// Enchantments keyed by namespaced ID (e.g. `"minecraft:silk_touch"`, `"minecraft:fortune"`).
    /// `IndexMap` preserves insertion order per `PORTING.md`.
    pub enchantments: IndexMap<String, i32>,
}

impl ToolStack {
    /// Empty hand (`ItemStack.EMPTY`).
    #[inline]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Standard tiered tool (pickaxe, axe, shovel, hoe).
    pub fn tool(tier: ToolTier, kind: ToolKind) -> Self {
        Self {
            item_id: Some(format!(
                "minecraft:{}_{}",
                tier.item_prefix(),
                kind.item_suffix()
            )),
            tool: Some(ToolSpec::for_tool(tier, kind)),
            enchantments: IndexMap::new(),
        }
    }

    /// Shears (`Items.SHEARS`).
    pub fn shears() -> Self {
        Self {
            item_id: Some("minecraft:shears".to_string()),
            tool: Some(ToolSpec::shears()),
            enchantments: IndexMap::new(),
        }
    }

    /// Tiered sword (e.g. `"minecraft:iron_sword"`).
    pub fn sword(tier: ToolTier) -> Self {
        Self {
            item_id: Some(format!("minecraft:{}_sword", tier.item_prefix())),
            tool: Some(ToolSpec::sword()),
            enchantments: IndexMap::new(),
        }
    }

    /// Arbitrary non-tool item (e.g. `"minecraft:stick"`).
    pub fn item(item_id: impl Into<String>) -> Self {
        Self {
            item_id: Some(normalize_id(&item_id.into())),
            tool: None,
            enchantments: IndexMap::new(),
        }
    }

    /// Attach an enchantment level (e.g. `("minecraft:fortune", 3)`).
    pub fn with_enchantment(mut self, id: impl Into<String>, level: i32) -> Self {
        self.enchantments.insert(normalize_id(&id.into()), level);
        self
    }

    /// Enchantment level on this tool, or `0` if absent.
    #[inline]
    pub fn enchantment_level(&self, id: &str) -> i32 {
        if let Some(&lvl) = self.enchantments.get(id) {
            return lvl;
        }
        let normalized = normalize_id(id);
        self.enchantments.get(&normalized).copied().unwrap_or(0)
    }

    /// Whether this tool has Silk Touch (`level >= 1`).
    #[inline]
    pub fn has_silk_touch(&self) -> bool {
        self.enchantment_level("minecraft:silk_touch") >= 1
    }

    /// Fortune enchantment level (`>= 0`).
    #[inline]
    pub fn fortune_level(&self) -> i32 {
        self.enchantment_level("minecraft:fortune").max(0)
    }

    /// Whether this item is `minecraft:shears`.
    #[inline]
    pub fn is_shears(&self) -> bool {
        self.item_id
            .as_deref()
            .is_some_and(|id| normalize_id(id) == "minecraft:shears")
    }

    /// Whether this item is a pickaxe (`minecraft:*_pickaxe`).
    #[inline]
    pub fn is_pickaxe(&self) -> bool {
        self.item_id
            .as_deref()
            .is_some_and(|id| strip_minecraft_ns(id).ends_with("_pickaxe"))
    }

    /// Paper `ItemStack.isCorrectToolForDrops(BlockState)`.
    #[inline]
    pub fn is_correct_tool_for_drops(&self, state: BlockState) -> bool {
        self.tool
            .as_ref()
            .is_some_and(|t| t.is_correct_for_drops(state))
    }
}

/// Paper `BlockBehaviour.BlockStateBase.requiresCorrectToolForDrops()` for MC 26.2.
///
/// Uses the generated block tags plus Paper's explicit `Blocks.java` registrations for
/// blocks that require a tool outside `minecraft:mineable/pickaxe` (e.g. `snow`,
/// `snow_block`, `cobweb`) and the pickaxe-mineable blocks that do not require a pickaxe
/// for drops (e.g. rails, ice, pistons, beacon, conduit, shulker boxes, amethyst buds,
/// brewing stand).
pub fn requires_correct_tool_for_drops(state: BlockState) -> bool {
    let name = strip_minecraft_ns(state.block().name());
    if matches!(
        name,
        "snow"
            | "snow_block"
            | "cobweb"
            | "command_block"
            | "chain_command_block"
            | "repeating_command_block"
            | "structure_block"
            | "jigsaw"
            | "sculk_shrieker"
    ) {
        return true;
    }

    if !state.is_in_tag("minecraft:mineable/pickaxe") {
        return false;
    }

    // Pickaxe-efficient blocks in Paper 26.2 that do NOT call requiresCorrectToolForDrops().
    if matches!(
        name,
        "rail"
            | "powered_rail"
            | "detector_rail"
            | "activator_rail"
            | "ice"
            | "packed_ice"
            | "blue_ice"
            | "frosted_ice"
            | "piston"
            | "sticky_piston"
            | "piston_head"
            | "moving_piston"
            | "beacon"
            | "conduit"
            | "brewing_stand"
            | "shulker_box"
            | "amethyst_cluster"
            | "large_amethyst_bud"
            | "medium_amethyst_bud"
            | "small_amethyst_bud"
            | "budding_amethyst"
            | "heavy_core"
            | "calibrated_sculk_sensor"
            | "sculk_sensor"
            | "sculk_catalyst"
            | "ender_chest"
            | "stone_button"
            | "polished_blackstone_button"
            | "decorated_pot"
    ) || name.ends_with("_shulker_box")
    {
        return false;
    }

    true
}

/// Paper `Player.hasCorrectToolForDrops(BlockState)`.
///
/// `!state.requiresCorrectToolForDrops() || tool.isCorrectToolForDrops(state)`
#[inline]
pub fn has_correct_tool_for_drops(state: BlockState, tool: &ToolStack) -> bool {
    !requires_correct_tool_for_drops(state) || tool.is_correct_tool_for_drops(state)
}

/// Paper `EnchantmentHelper.processBlockExperience(ServerLevel, ItemStack, int)`.
///
/// In vanilla/Paper 26.2, `minecraft:silk_touch` registers a `minecraft:block_experience`
/// effect setting the value to `0.0`.
#[inline]
pub fn process_block_experience(tool: &ToolStack, raw_experience: i32) -> i32 {
    if tool.has_silk_touch() {
        0
    } else {
        raw_experience.max(0)
    }
}

/// Paper `Block.getExpDrop(BlockState, ServerLevel, BlockPos, ItemStack, boolean)`.
///
/// Mirrors `DropExperienceBlock`, `RedStoneOreBlock`, `SpawnerBlock`, `SculkBlock`,
/// `SculkCatalystBlock`, `SculkShriekerBlock`, and `SculkSensorBlock` in Paper 26.2.
pub fn block_exp_drop<R: RandomSource>(
    state: BlockState,
    tool: &ToolStack,
    drop_experience: bool,
    random: &mut R,
) -> i32 {
    if !drop_experience {
        return 0;
    }

    let name = strip_minecraft_ns(state.block().name());
    match name {
        // SpawnerBlock.getExpDrop computes directly without EnchantmentHelper.processBlockExperience.
        "spawner" => 15_i32
            .wrapping_add(random.next_int_bound(15))
            .wrapping_add(random.next_int_bound(15)),
        "coal_ore" | "deepslate_coal_ore" => {
            let sampled = mth::next_int(random, 0, 2);
            process_block_experience(tool, sampled)
        }
        "lapis_ore" | "deepslate_lapis_ore" | "nether_quartz_ore" => {
            let sampled = mth::next_int(random, 2, 5);
            process_block_experience(tool, sampled)
        }
        "diamond_ore" | "deepslate_diamond_ore" | "emerald_ore" | "deepslate_emerald_ore" => {
            let sampled = mth::next_int(random, 3, 7);
            process_block_experience(tool, sampled)
        }
        "nether_gold_ore" => {
            let sampled = mth::next_int(random, 0, 1);
            process_block_experience(tool, sampled)
        }
        "redstone_ore" | "deepslate_redstone_ore" => {
            // RedStoneOreBlock: 1 + random.nextInt(5) == UniformInt(1, 5)
            let sampled = 1_i32.wrapping_add(random.next_int_bound(5));
            process_block_experience(tool, sampled)
        }
        "sculk_catalyst" | "sculk_shrieker" | "sculk_sensor" | "calibrated_sculk_sensor" => {
            process_block_experience(tool, 5)
        }
        "sculk" => process_block_experience(tool, 1),
        _ => 0,
    }
}

/// A resolved item stack dropped by breaking a block.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BlockDropStack {
    /// Namespaced item ID (e.g. `"minecraft:cobblestone"`).
    pub item_id: String,
    /// Positive stack count (stacks with `count <= 0` are suppressed per `Block.popResource`).
    pub count: i32,
}

impl BlockDropStack {
    #[inline]
    pub fn new(item_id: impl Into<String>, count: i32) -> Self {
        Self {
            item_id: normalize_id(&item_id.into()),
            count,
        }
    }
}

/// Loot evaluation context matching Paper `LootContextParamSets.BLOCK`.
#[derive(Clone, PartialEq, Debug)]
pub struct BlockDropContext {
    pub block_state: BlockState,
    pub pos: BlockPos,
    pub tool: ToolStack,
    /// `LootContextParams.EXPLOSION_RADIUS`: `None` for player block breaking,
    /// `Some(radius)` for explosion-caused block drops.
    pub explosion_radius: Option<f32>,
}

impl BlockDropContext {
    pub fn new(block_state: BlockState, pos: BlockPos, tool: ToolStack) -> Self {
        Self {
            block_state,
            pos,
            tool,
            explosion_radius: None,
        }
    }

    pub fn with_explosion_radius(mut self, radius: f32) -> Self {
        self.explosion_radius = Some(radius);
        self
    }
}

/// Server-authoritative block-break context mirroring `ServerPlayerGameMode.destroyBlock`.
#[derive(Clone, PartialEq, Debug)]
pub struct BlockBreakContext {
    pub game_mode: GameType,
    /// Paper `Player.getAbilities().instabuild` (`Player.preventsBlockDrops()`).
    pub instabuild: bool,
    pub block_state: BlockState,
    pub pos: BlockPos,
    pub tool: ToolStack,
    /// Paper `GameRules.BLOCK_DROPS` (default `true`).
    pub block_drops_rule: bool,
    /// Paper `BlockBreakEvent.isDropItems()` (default `true`).
    pub event_drop_items: bool,
    /// Paper `BlockBreakEvent` experience drop enablement (default `true`).
    pub event_drop_exp: bool,
    /// Paper `boolean changed = this.level.removeBlock(pos, false)` (default `true`).
    pub block_removed: bool,
    /// Optional explosion radius (`None` for player mining).
    pub explosion_radius: Option<f32>,
}

impl BlockBreakContext {
    /// Construct a standard player block-break context for the given game mode.
    pub fn for_player(
        game_mode: GameType,
        block_state: BlockState,
        pos: BlockPos,
        tool: ToolStack,
    ) -> Self {
        Self {
            game_mode,
            instabuild: game_mode.is_creative(),
            block_state,
            pos,
            tool,
            block_drops_rule: true,
            event_drop_items: true,
            event_drop_exp: true,
            block_removed: true,
            explosion_radius: None,
        }
    }

    /// Paper `Player.preventsBlockDrops()`.
    #[inline]
    pub const fn prevents_block_drops(&self) -> bool {
        self.instabuild
    }
}

/// Output of server-authoritative block-break drop resolution.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct BlockBreakResolution {
    /// Whether `Player.preventsBlockDrops()` short-circuited the drop path.
    pub prevents_block_drops: bool,
    /// Result of `Player.hasCorrectToolForDrops(adjustedState)`.
    pub has_correct_tool_for_drops: bool,
    /// Non-empty item drops to spawn via `Block.popResource`.
    pub items: Vec<BlockDropStack>,
    /// Experience to award via `Block.popExperience` (`0` if suppressed).
    pub experience: i32,
}

/// Resolve drops and experience for a player breaking a block, following the exact
/// Paper `ServerPlayerGameMode.destroyBlock` -> `Block.playerDestroy` -> `Block.dropResources`
/// -> `Block.popResource` / `Block.popExperience` control flow.
pub fn resolve_block_break<R: RandomSource>(
    ctx: &BlockBreakContext,
    random: &mut R,
) -> BlockBreakResolution {
    resolve_block_break_with_table(ctx, None, random)
}

/// Same as [`resolve_block_break`], optionally overriding the block's loot table with
/// a parsed Paper JSON [`BlockLootTable`].
pub fn resolve_block_break_with_table<R: RandomSource>(
    ctx: &BlockBreakContext,
    override_table: Option<&BlockLootTable>,
    random: &mut R,
) -> BlockBreakResolution {
    if ctx.prevents_block_drops() {
        return BlockBreakResolution {
            prevents_block_drops: true,
            has_correct_tool_for_drops: false,
            items: Vec::new(),
            experience: 0,
        };
    }

    let can_destroy = has_correct_tool_for_drops(ctx.block_state, &ctx.tool);
    if !ctx.block_removed || !can_destroy {
        return BlockBreakResolution {
            prevents_block_drops: false,
            has_correct_tool_for_drops: can_destroy,
            items: Vec::new(),
            experience: 0,
        };
    }

    let drop_ctx = BlockDropContext {
        block_state: ctx.block_state,
        pos: ctx.pos,
        tool: ctx.tool.clone(),
        explosion_radius: ctx.explosion_radius,
    };

    let mut items = if ctx.event_drop_items && ctx.block_drops_rule {
        match override_table {
            Some(table) => table.evaluate(&drop_ctx, random),
            None => resolve_builtin_block_drops(&drop_ctx, random),
        }
    } else {
        Vec::new()
    };
    // Paper Block.popResource ignores empty stacks.
    items.retain(|stack| stack.count > 0);

    // Paper Block.popExperience also gates on GameRules.BLOCK_DROPS.
    let experience = if ctx.block_drops_rule {
        block_exp_drop(ctx.block_state, &ctx.tool, ctx.event_drop_exp, random)
    } else {
        0
    };

    BlockBreakResolution {
        prevents_block_drops: false,
        has_correct_tool_for_drops: true,
        items,
        experience,
    }
}

// ---------------------------------------------------------------------------
// Paper 26.2 Fortune & loot math primitives
// ---------------------------------------------------------------------------

/// Paper `ApplyBonusCount.OreDrops.calculateNewCount(random, count, level)`.
#[inline]
pub fn apply_bonus_ore_drops<R: RandomSource>(random: &mut R, count: i32, level: i32) -> i32 {
    if level > 0 {
        let mut bonus = random.next_int_bound(level.wrapping_add(2)).wrapping_sub(1);
        if bonus < 0 {
            bonus = 0;
        }
        count.wrapping_mul(bonus.wrapping_add(1))
    } else {
        count
    }
}

/// Paper `ApplyBonusCount.UniformBonusCount.calculateNewCount(random, count, level)`.
#[inline]
pub fn apply_bonus_uniform<R: RandomSource>(
    random: &mut R,
    count: i32,
    level: i32,
    bonus_multiplier: i32,
) -> i32 {
    let bound = bonus_multiplier.wrapping_mul(level).wrapping_add(1);
    if bound <= 0 {
        return count;
    }
    count.wrapping_add(random.next_int_bound(bound))
}

/// Paper `ApplyBonusCount.BinomialWithBonusCount.calculateNewCount(random, count, level)`.
#[inline]
pub fn apply_bonus_binomial<R: RandomSource>(
    random: &mut R,
    mut count: i32,
    level: i32,
    extra_rounds: i32,
    probability: f32,
) -> i32 {
    let rounds = level.wrapping_add(extra_rounds);
    for _ in 0..rounds {
        if random.next_float() < probability {
            count = count.wrapping_add(1);
        }
    }
    count
}

/// Paper `BonusLevelTableCondition.test(LootContext)` (`minecraft:table_bonus`).
#[inline]
pub fn test_table_bonus<R: RandomSource>(random: &mut R, level: i32, chances: &[f32]) -> bool {
    if chances.is_empty() {
        return false;
    }
    let idx = (level.max(0) as usize).min(chances.len() - 1);
    random.next_float() < chances[idx]
}

/// Paper `ExplosionCondition.test(LootContext)` (`minecraft:survives_explosion`).
#[inline]
pub fn survives_explosion<R: RandomSource>(random: &mut R, explosion_radius: Option<f32>) -> bool {
    match explosion_radius {
        Some(radius) if radius > 0.0 => random.next_float() <= 1.0_f32 / radius,
        _ => true,
    }
}

/// Paper `ApplyExplosionDecay.run(ItemStack, LootContext)` (`minecraft:explosion_decay`).
#[inline]
pub fn apply_explosion_decay<R: RandomSource>(
    random: &mut R,
    count: i32,
    explosion_radius: Option<f32>,
) -> i32 {
    let Some(radius) = explosion_radius else {
        return count;
    };
    if radius <= 0.0 || count <= 0 {
        return count;
    }
    let survival_chance = 1.0_f32 / radius;
    let mut surviving = 0_i32;
    for _ in 0..count {
        if random.next_float() <= survival_chance {
            surviving = surviving.wrapping_add(1);
        }
    }
    surviving
}

// ---------------------------------------------------------------------------
// Built-in Paper 26.2 ordinary survival block loot resolver
// ---------------------------------------------------------------------------

const GRAVEL_FLINT_CHANCES: &[f32] = &[0.1, 0.14285715, 0.25, 1.0];
const DEFAULT_SAPLING_CHANCES: &[f32] = &[0.05, 0.0625, 0.083333336, 0.1];
const JUNGLE_SAPLING_CHANCES: &[f32] = &[0.025, 0.027777778, 0.03125, 0.041666668, 0.1];
const LEAF_STICK_CHANCES: &[f32] = &[0.02, 0.022222223, 0.025, 0.033333335, 0.1];
const LEAF_APPLE_CHANCES: &[f32] = &[0.005, 0.0055555557, 0.00625, 0.008333334, 0.025];

/// Built-in Paper 26.2 block loot resolver for ordinary survival blocks.
///
/// Mirrors `data/minecraft/loot_table/blocks/*.json` in Paper 26.2.
pub fn resolve_builtin_block_drops<R: RandomSource>(
    ctx: &BlockDropContext,
    random: &mut R,
) -> Vec<BlockDropStack> {
    let full_name = ctx.block_state.block().name();
    let name = strip_minecraft_ns(full_name);
    let silk = ctx.tool.has_silk_touch();
    let fortune = ctx.tool.fortune_level();

    // Blocks with `noLootTable()` in Paper `Blocks.java`.
    if is_no_loot_table_block(name) {
        return Vec::new();
    }

    // Silk-touch-only blocks (glass, stained glass, panes, ice, packed_ice, blue_ice, sculk, etc.).
    if is_silk_touch_only_block(name) {
        return if silk {
            vec![BlockDropStack::new(full_name, 1)]
        } else {
            Vec::new()
        };
    }

    // Infested blocks: silk touch drops the host stone block; normal break drops nothing.
    if let Some(host) = name.strip_prefix("infested_") {
        return if silk {
            vec![BlockDropStack::new(format!("minecraft:{host}"), 1)]
        } else {
            Vec::new()
        };
    }

    // Leaf blocks (`minecraft:leaves`).
    if ctx.block_state.is_in_tag("minecraft:leaves") {
        return resolve_leaves_drops(full_name, name, ctx, random);
    }

    match name {
        // Silk touch -> self; normal -> alternate single drop (with survives_explosion).
        "stone" => silk_or_single(full_name, "minecraft:cobblestone", 1, ctx, random),
        "deepslate" => silk_or_single(full_name, "minecraft:cobbled_deepslate", 1, ctx, random),
        "grass_block" | "mycelium" | "podzol" => {
            silk_or_single(full_name, "minecraft:dirt", 1, ctx, random)
        }
        "crimson_nylium" | "warped_nylium" => {
            silk_or_single(full_name, "minecraft:netherrack", 1, ctx, random)
        }
        "farmland" | "dirt_path" => {
            if survives_explosion(random, ctx.explosion_radius) {
                vec![BlockDropStack::new("minecraft:dirt", 1)]
            } else {
                Vec::new()
            }
        }
        "bookshelf" => silk_or_count_with_decay(full_name, "minecraft:book", 3, ctx, random),
        "ender_chest" => silk_or_count_with_decay(full_name, "minecraft:obsidian", 8, ctx, random),
        "campfire" => silk_or_count_with_decay(full_name, "minecraft:charcoal", 2, ctx, random),
        "soul_campfire" => {
            silk_or_count_with_decay(full_name, "minecraft:soul_soil", 1, ctx, random)
        }
        "clay" => silk_or_count_with_decay(full_name, "minecraft:clay_ball", 4, ctx, random),
        "snow_block" => silk_or_count_with_decay(full_name, "minecraft:snowball", 4, ctx, random),
        "snow" => resolve_snow_layer_drops(ctx, random),
        "cobweb" => {
            if ctx.tool.is_shears() || silk {
                vec![BlockDropStack::new("minecraft:cobweb", 1)]
            } else if survives_explosion(random, ctx.explosion_radius) {
                vec![BlockDropStack::new("minecraft:string", 1)]
            } else {
                Vec::new()
            }
        }

        // Single-base ores with `ore_drops` Fortune.
        "coal_ore" | "deepslate_coal_ore" => {
            ore_drop(full_name, "minecraft:coal", 1, 1, ctx, random)
        }
        "iron_ore" | "deepslate_iron_ore" => {
            ore_drop(full_name, "minecraft:raw_iron", 1, 1, ctx, random)
        }
        "gold_ore" | "deepslate_gold_ore" => {
            ore_drop(full_name, "minecraft:raw_gold", 1, 1, ctx, random)
        }
        "diamond_ore" | "deepslate_diamond_ore" => {
            ore_drop(full_name, "minecraft:diamond", 1, 1, ctx, random)
        }
        "emerald_ore" | "deepslate_emerald_ore" => {
            ore_drop(full_name, "minecraft:emerald", 1, 1, ctx, random)
        }
        "nether_quartz_ore" => ore_drop(full_name, "minecraft:quartz", 1, 1, ctx, random),

        // Multi-base ores with `ore_drops` Fortune.
        "copper_ore" | "deepslate_copper_ore" => {
            ore_drop(full_name, "minecraft:raw_copper", 2, 5, ctx, random)
        }
        "lapis_ore" | "deepslate_lapis_ore" => {
            ore_drop(full_name, "minecraft:lapis_lazuli", 4, 9, ctx, random)
        }
        "nether_gold_ore" => ore_drop(full_name, "minecraft:gold_nugget", 2, 6, ctx, random),

        // Uniform-bonus ores and blocks.
        "redstone_ore" | "deepslate_redstone_ore" => {
            if silk {
                vec![BlockDropStack::new(full_name, 1)]
            } else {
                let base = mth::next_int(random, 4, 5);
                let bonused = apply_bonus_uniform(random, base, fortune, 1);
                let final_count = apply_explosion_decay(random, bonused, ctx.explosion_radius);
                non_empty_drop("minecraft:redstone", final_count)
            }
        }
        "glowstone" => {
            if silk {
                vec![BlockDropStack::new(full_name, 1)]
            } else {
                let base = mth::next_int(random, 2, 4);
                let bonused = apply_bonus_uniform(random, base, fortune, 1).clamp(1, 4);
                let final_count = apply_explosion_decay(random, bonused, ctx.explosion_radius);
                non_empty_drop("minecraft:glowstone_dust", final_count)
            }
        }
        "sea_lantern" => {
            if silk {
                vec![BlockDropStack::new(full_name, 1)]
            } else {
                let base = mth::next_int(random, 2, 3);
                let bonused = apply_bonus_uniform(random, base, fortune, 1).clamp(1, 5);
                let final_count = apply_explosion_decay(random, bonused, ctx.explosion_radius);
                non_empty_drop("minecraft:prismarine_crystals", final_count)
            }
        }
        "melon" => {
            if silk {
                vec![BlockDropStack::new(full_name, 1)]
            } else {
                let base = mth::next_int(random, 3, 7);
                let bonused = apply_bonus_uniform(random, base, fortune, 1).clamp(0, 9);
                let final_count = apply_explosion_decay(random, bonused, ctx.explosion_radius);
                non_empty_drop("minecraft:melon_slice", final_count)
            }
        }

        // Table-bonus blocks.
        "gravel" => {
            if silk {
                vec![BlockDropStack::new(full_name, 1)]
            } else if survives_explosion(random, ctx.explosion_radius)
                && test_table_bonus(random, fortune, GRAVEL_FLINT_CHANCES)
            {
                vec![BlockDropStack::new("minecraft:flint", 1)]
            } else {
                vec![BlockDropStack::new("minecraft:gravel", 1)]
            }
        }
        "gilded_blackstone" => {
            if silk {
                vec![BlockDropStack::new(full_name, 1)]
            } else if survives_explosion(random, ctx.explosion_radius)
                && test_table_bonus(random, fortune, GRAVEL_FLINT_CHANCES)
            {
                let count = mth::next_int(random, 2, 5);
                let final_count = apply_explosion_decay(random, count, ctx.explosion_radius);
                non_empty_drop("minecraft:gold_nugget", final_count)
            } else {
                vec![BlockDropStack::new("minecraft:gilded_blackstone", 1)]
            }
        }
        "amethyst_cluster" => {
            if silk {
                vec![BlockDropStack::new(full_name, 1)]
            } else if ctx.tool.is_pickaxe() {
                let bonused = apply_bonus_ore_drops(random, 4, fortune);
                non_empty_drop("minecraft:amethyst_shard", bonused)
            } else {
                let final_count = apply_explosion_decay(random, 2, ctx.explosion_radius);
                non_empty_drop("minecraft:amethyst_shard", final_count)
            }
        }

        // Grass / fern / wheat.
        "short_grass" | "fern" => {
            if ctx.tool.is_shears() {
                vec![BlockDropStack::new(full_name, 1)]
            } else if random.next_float() < 0.125 {
                let bonused = apply_bonus_uniform(random, 1, fortune, 2);
                let final_count = apply_explosion_decay(random, bonused, ctx.explosion_radius);
                non_empty_drop("minecraft:wheat_seeds", final_count)
            } else {
                Vec::new()
            }
        }
        "wheat" => resolve_wheat_drops(ctx, random),

        // Slabs: `type=double` drops 2, single drops 1.
        _ if name.ends_with("_slab") => {
            let count = if block_state_property_equals(ctx.block_state, "type", "double") {
                2
            } else {
                1
            };
            let final_count = apply_explosion_decay(random, count, ctx.explosion_radius);
            non_empty_drop(full_name, final_count)
        }

        // Default ordinary survival block: drops itself if it survives explosion.
        _ => {
            // Double-block items (doors, tall flowers, beds) only drop from the lower/primary half
            // when `half` property is present.
            if block_state_property_equals(ctx.block_state, "half", "upper") {
                return Vec::new();
            }
            if survives_explosion(random, ctx.explosion_radius) {
                vec![BlockDropStack::new(full_name, 1)]
            } else {
                Vec::new()
            }
        }
    }
}

fn is_no_loot_table_block(name: &str) -> bool {
    matches!(
        name,
        "air"
            | "cave_air"
            | "void_air"
            | "bedrock"
            | "water"
            | "lava"
            | "bubble_column"
            | "fire"
            | "soul_fire"
            | "barrier"
            | "command_block"
            | "chain_command_block"
            | "repeating_command_block"
            | "structure_block"
            | "structure_void"
            | "jigsaw"
            | "light"
            | "end_portal"
            | "end_portal_frame"
            | "end_gateway"
            | "nether_portal"
            | "spawner"
            | "trial_spawner"
            | "vault"
            | "reinforced_deepslate"
            | "budding_amethyst"
            | "frogspawn"
            | "moving_piston"
            | "piston_head"
    )
}

fn is_silk_touch_only_block(name: &str) -> bool {
    matches!(
        name,
        "glass"
            | "tinted_glass"
            | "glass_pane"
            | "ice"
            | "packed_ice"
            | "blue_ice"
            | "sculk"
            | "sculk_catalyst"
            | "sculk_shrieker"
            | "sculk_vein"
            | "large_amethyst_bud"
            | "medium_amethyst_bud"
            | "small_amethyst_bud"
            | "turtle_egg"
            | "chiseled_bookshelf"
    ) || name.ends_with("_stained_glass")
        || name.ends_with("_stained_glass_pane")
}

fn silk_or_single<R: RandomSource>(
    self_id: &str,
    normal_id: &str,
    count: i32,
    ctx: &BlockDropContext,
    random: &mut R,
) -> Vec<BlockDropStack> {
    if ctx.tool.has_silk_touch() {
        vec![BlockDropStack::new(self_id, 1)]
    } else if survives_explosion(random, ctx.explosion_radius) {
        non_empty_drop(normal_id, count)
    } else {
        Vec::new()
    }
}

fn silk_or_count_with_decay<R: RandomSource>(
    self_id: &str,
    normal_id: &str,
    count: i32,
    ctx: &BlockDropContext,
    random: &mut R,
) -> Vec<BlockDropStack> {
    if ctx.tool.has_silk_touch() {
        vec![BlockDropStack::new(self_id, 1)]
    } else {
        let final_count = apply_explosion_decay(random, count, ctx.explosion_radius);
        non_empty_drop(normal_id, final_count)
    }
}

fn ore_drop<R: RandomSource>(
    ore_id: &str,
    drop_id: &str,
    min_count: i32,
    max_count: i32,
    ctx: &BlockDropContext,
    random: &mut R,
) -> Vec<BlockDropStack> {
    if ctx.tool.has_silk_touch() {
        return vec![BlockDropStack::new(ore_id, 1)];
    }
    let base = if min_count == max_count {
        min_count
    } else {
        mth::next_int(random, min_count, max_count)
    };
    let bonused = apply_bonus_ore_drops(random, base, ctx.tool.fortune_level());
    let final_count = apply_explosion_decay(random, bonused, ctx.explosion_radius);
    non_empty_drop(drop_id, final_count)
}

fn resolve_snow_layer_drops<R: RandomSource>(
    ctx: &BlockDropContext,
    random: &mut R,
) -> Vec<BlockDropStack> {
    let layers = block_state_int_property(ctx.block_state, "layers").unwrap_or(1);
    if ctx.tool.has_silk_touch() {
        if layers >= 8 {
            vec![BlockDropStack::new("minecraft:snow_block", 1)]
        } else {
            non_empty_drop("minecraft:snow", layers)
        }
    } else {
        let final_count = apply_explosion_decay(random, layers, ctx.explosion_radius);
        non_empty_drop("minecraft:snowball", final_count)
    }
}

fn resolve_wheat_drops<R: RandomSource>(
    ctx: &BlockDropContext,
    random: &mut R,
) -> Vec<BlockDropStack> {
    let mature = block_state_int_property(ctx.block_state, "age") == Some(7);
    let mut out = Vec::new();
    if mature {
        let wheat_count = apply_explosion_decay(random, 1, ctx.explosion_radius);
        if wheat_count > 0 {
            out.push(BlockDropStack::new("minecraft:wheat", wheat_count));
        }
        let seeds = apply_bonus_binomial(random, 1, ctx.tool.fortune_level(), 3, 0.5714286_f32);
        let final_seeds = apply_explosion_decay(random, seeds, ctx.explosion_radius);
        if final_seeds > 0 {
            out.push(BlockDropStack::new("minecraft:wheat_seeds", final_seeds));
        }
    } else {
        let final_seeds = apply_explosion_decay(random, 1, ctx.explosion_radius);
        if final_seeds > 0 {
            out.push(BlockDropStack::new("minecraft:wheat_seeds", final_seeds));
        }
    }
    out
}

fn resolve_leaves_drops<R: RandomSource>(
    full_name: &str,
    name: &str,
    ctx: &BlockDropContext,
    random: &mut R,
) -> Vec<BlockDropStack> {
    if ctx.tool.is_shears() || ctx.tool.has_silk_touch() {
        return vec![BlockDropStack::new(full_name, 1)];
    }

    let fortune = ctx.tool.fortune_level();
    let mut out = Vec::new();

    // Pool 1: Sapling drop.
    let (sapling, sapling_chances) = match name {
        "oak_leaves" => (Some("minecraft:oak_sapling"), DEFAULT_SAPLING_CHANCES),
        "spruce_leaves" => (Some("minecraft:spruce_sapling"), DEFAULT_SAPLING_CHANCES),
        "birch_leaves" => (Some("minecraft:birch_sapling"), DEFAULT_SAPLING_CHANCES),
        "jungle_leaves" => (Some("minecraft:jungle_sapling"), JUNGLE_SAPLING_CHANCES),
        "acacia_leaves" => (Some("minecraft:acacia_sapling"), DEFAULT_SAPLING_CHANCES),
        "dark_oak_leaves" => (Some("minecraft:dark_oak_sapling"), DEFAULT_SAPLING_CHANCES),
        "cherry_leaves" => (Some("minecraft:cherry_sapling"), DEFAULT_SAPLING_CHANCES),
        "pale_oak_leaves" => (Some("minecraft:pale_oak_sapling"), DEFAULT_SAPLING_CHANCES),
        "azalea_leaves" => (Some("minecraft:azalea"), DEFAULT_SAPLING_CHANCES),
        "flowering_azalea_leaves" => (Some("minecraft:flowering_azalea"), DEFAULT_SAPLING_CHANCES),
        _ => (None, DEFAULT_SAPLING_CHANCES),
    };
    if let Some(sapling_id) = sapling
        && survives_explosion(random, ctx.explosion_radius)
        && test_table_bonus(random, fortune, sapling_chances)
    {
        out.push(BlockDropStack::new(sapling_id, 1));
    }

    // Pool 2: Stick drop (1..=2 with explosion_decay).
    if test_table_bonus(random, fortune, LEAF_STICK_CHANCES) {
        let base = mth::next_int(random, 1, 2);
        let final_count = apply_explosion_decay(random, base, ctx.explosion_radius);
        if final_count > 0 {
            out.push(BlockDropStack::new("minecraft:stick", final_count));
        }
    }

    // Pool 3: Apple drop (oak_leaves and dark_oak_leaves only).
    if matches!(name, "oak_leaves" | "dark_oak_leaves")
        && survives_explosion(random, ctx.explosion_radius)
        && test_table_bonus(random, fortune, LEAF_APPLE_CHANCES)
    {
        out.push(BlockDropStack::new("minecraft:apple", 1));
    }

    out
}

#[inline]
fn non_empty_drop(item_id: &str, count: i32) -> Vec<BlockDropStack> {
    if count > 0 {
        vec![BlockDropStack::new(item_id, count)]
    } else {
        Vec::new()
    }
}

// ---------------------------------------------------------------------------
// Paper 26.2 JSON Block Loot Table Parser & Evaluator
// ---------------------------------------------------------------------------

/// Error returned when parsing a Paper 26.2 block loot table JSON.
#[derive(Debug, Error)]
pub enum LootTableError {
    #[error("invalid loot table JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid loot table structure: {0}")]
    Schema(String),
}

/// A parsed Paper 26.2 block loot table (`data/minecraft/loot_table/blocks/*.json`).
#[derive(Clone, PartialEq, Debug)]
pub struct BlockLootTable {
    pub pools: Vec<LootPool>,
    pub functions: Vec<LootFunction>,
}

/// A single pool within a [`BlockLootTable`].
#[derive(Clone, PartialEq, Debug)]
pub struct LootPool {
    pub rolls: NumberProvider,
    pub bonus_rolls: NumberProvider,
    pub conditions: Vec<LootCondition>,
    pub entries: Vec<LootEntry>,
    pub functions: Vec<LootFunction>,
}

/// Supported Paper 26.2 block loot entry types.
#[derive(Clone, PartialEq, Debug)]
pub enum LootEntry {
    Item {
        name: String,
        conditions: Vec<LootCondition>,
        functions: Vec<LootFunction>,
    },
    Alternatives {
        children: Vec<LootEntry>,
        conditions: Vec<LootCondition>,
    },
    Group {
        children: Vec<LootEntry>,
        conditions: Vec<LootCondition>,
    },
    Sequence {
        children: Vec<LootEntry>,
        conditions: Vec<LootCondition>,
    },
    Empty {
        conditions: Vec<LootCondition>,
    },
}

/// Supported Paper 26.2 block loot conditions.
#[derive(Clone, PartialEq, Debug)]
pub enum LootCondition {
    SurvivesExplosion,
    BlockStateProperty {
        block: String,
        properties: IndexMap<String, String>,
    },
    MatchTool {
        items: Option<Vec<String>>,
        enchantments: Vec<EnchantmentPredicate>,
    },
    AnyOf(Vec<LootCondition>),
    AllOf(Vec<LootCondition>),
    Inverted(Box<LootCondition>),
    TableBonus {
        enchantment: String,
        chances: Vec<f32>,
    },
    RandomChance(f32),
    /// Conditions whose runtime context lives outside `rivet-world` (e.g. `location_check`,
    /// `entity_properties`); conservatively evaluates to `false` unless explicitly satisfied.
    Unsupported(String),
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct EnchantmentPredicate {
    pub enchantment: String,
    pub min_level: Option<i32>,
    pub max_level: Option<i32>,
}

/// Supported Paper 26.2 block loot functions.
#[derive(Clone, PartialEq, Debug)]
pub enum LootFunction {
    SetCount {
        count: NumberProvider,
        add: bool,
        conditions: Vec<LootCondition>,
    },
    ApplyBonus {
        enchantment: String,
        formula: BonusFormula,
        conditions: Vec<LootCondition>,
    },
    LimitCount {
        min: Option<i32>,
        max: Option<i32>,
        conditions: Vec<LootCondition>,
    },
    ExplosionDecay {
        conditions: Vec<LootCondition>,
    },
    /// Pass-through metadata functions (`copy_components`, `copy_state`) that do not alter item counts.
    NoOp,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum BonusFormula {
    OreDrops,
    UniformBonusCount { bonus_multiplier: i32 },
    BinomialWithBonusCount { extra: i32, probability: f32 },
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum NumberProvider {
    Constant(f32),
    Uniform { min: f32, max: f32 },
    Binomial { n: i32, p: f32 },
}

impl NumberProvider {
    pub fn get_int<R: RandomSource>(self, random: &mut R) -> i32 {
        match self {
            Self::Constant(v) => v.round() as i32,
            Self::Uniform { min, max } => mth::next_int(random, min as i32, max as i32),
            Self::Binomial { n, p } => {
                let mut count = 0_i32;
                for _ in 0..n {
                    if random.next_float() < p {
                        count = count.wrapping_add(1);
                    }
                }
                count
            }
        }
    }
}

impl BlockLootTable {
    /// Parse a Paper 26.2 block loot table from a JSON string.
    pub fn from_json(json: &str) -> Result<Self, LootTableError> {
        let value: Value = serde_json::from_str(json)?;
        Self::from_value(&value)
    }

    /// Parse a Paper 26.2 block loot table from a `serde_json::Value`.
    pub fn from_value(value: &Value) -> Result<Self, LootTableError> {
        let obj = value
            .as_object()
            .ok_or_else(|| LootTableError::Schema("root must be a JSON object".into()))?;

        let mut pools = Vec::new();
        if let Some(pool_arr) = obj.get("pools").and_then(Value::as_array) {
            for pool_val in pool_arr {
                pools.push(parse_pool(pool_val)?);
            }
        }

        let functions = parse_functions(obj.get("functions"))?;
        Ok(Self { pools, functions })
    }

    /// Evaluate this loot table for the given [`BlockDropContext`], producing non-empty drops.
    pub fn evaluate<R: RandomSource>(
        &self,
        ctx: &BlockDropContext,
        random: &mut R,
    ) -> Vec<BlockDropStack> {
        let mut drops = Vec::new();
        for pool in &self.pools {
            if !all_conditions_pass(&pool.conditions, ctx, random) {
                continue;
            }
            let rolls = pool.rolls.get_int(random);
            for _ in 0..rolls {
                let start = drops.len();
                for entry in &pool.entries {
                    expand_entry(entry, ctx, random, &mut drops);
                }
                for stack in &mut drops[start..] {
                    apply_functions(&pool.functions, stack, ctx, random);
                }
            }
        }

        for stack in &mut drops {
            apply_functions(&self.functions, stack, ctx, random);
        }
        drops.retain(|s| s.count > 0);
        drops
    }
}

fn expand_entry<R: RandomSource>(
    entry: &LootEntry,
    ctx: &BlockDropContext,
    random: &mut R,
    out: &mut Vec<BlockDropStack>,
) -> bool {
    match entry {
        LootEntry::Item {
            name,
            conditions,
            functions,
        } => {
            if !all_conditions_pass(conditions, ctx, random) {
                return false;
            }
            let mut stack = BlockDropStack::new(name, 1);
            apply_functions(functions, &mut stack, ctx, random);
            if stack.count > 0 {
                out.push(stack);
            }
            true
        }
        LootEntry::Alternatives {
            children,
            conditions,
        } => {
            if !all_conditions_pass(conditions, ctx, random) {
                return false;
            }
            for child in children {
                if expand_entry(child, ctx, random, out) {
                    return true;
                }
            }
            false
        }
        LootEntry::Group {
            children,
            conditions,
        } => {
            if !all_conditions_pass(conditions, ctx, random) {
                return false;
            }
            let mut any = false;
            for child in children {
                if expand_entry(child, ctx, random, out) {
                    any = true;
                }
            }
            any
        }
        LootEntry::Sequence {
            children,
            conditions,
        } => {
            if !all_conditions_pass(conditions, ctx, random) {
                return false;
            }
            for child in children {
                if !expand_entry(child, ctx, random, out) {
                    return false;
                }
            }
            true
        }
        LootEntry::Empty { conditions } => all_conditions_pass(conditions, ctx, random),
    }
}

fn all_conditions_pass<R: RandomSource>(
    conditions: &[LootCondition],
    ctx: &BlockDropContext,
    random: &mut R,
) -> bool {
    conditions.iter().all(|c| eval_condition(c, ctx, random))
}

fn eval_condition<R: RandomSource>(
    cond: &LootCondition,
    ctx: &BlockDropContext,
    random: &mut R,
) -> bool {
    match cond {
        LootCondition::SurvivesExplosion => survives_explosion(random, ctx.explosion_radius),
        LootCondition::BlockStateProperty { block, properties } => {
            if !block_name_matches(ctx.block_state.block().name(), block) {
                return false;
            }
            properties
                .iter()
                .all(|(k, v)| block_state_property_equals(ctx.block_state, k, v))
        }
        LootCondition::MatchTool {
            items,
            enchantments,
        } => {
            if let Some(allowed_items) = items {
                let Some(tool_id) = ctx.tool.item_id.as_deref() else {
                    return false;
                };
                let normalized_tool = normalize_id(tool_id);
                let matched = allowed_items.iter().any(|pattern| {
                    if let Some(tag) = pattern.strip_prefix('#') {
                        matches_item_tag(&ctx.tool, tag)
                    } else {
                        normalize_id(pattern) == normalized_tool
                    }
                });
                if !matched {
                    return false;
                }
            }
            for ench in enchantments {
                let lvl = ctx.tool.enchantment_level(&ench.enchantment);
                if let Some(min) = ench.min_level
                    && lvl < min
                {
                    return false;
                }
                if let Some(max) = ench.max_level
                    && lvl > max
                {
                    return false;
                }
            }
            true
        }
        LootCondition::AnyOf(terms) => terms.iter().any(|t| eval_condition(t, ctx, random)),
        LootCondition::AllOf(terms) => terms.iter().all(|t| eval_condition(t, ctx, random)),
        LootCondition::Inverted(inner) => !eval_condition(inner, ctx, random),
        LootCondition::TableBonus {
            enchantment,
            chances,
        } => {
            let level = ctx.tool.enchantment_level(enchantment);
            test_table_bonus(random, level, chances)
        }
        LootCondition::RandomChance(chance) => random.next_float() < *chance,
        LootCondition::Unsupported(_) => false,
    }
}

fn apply_functions<R: RandomSource>(
    functions: &[LootFunction],
    stack: &mut BlockDropStack,
    ctx: &BlockDropContext,
    random: &mut R,
) {
    for func in functions {
        match func {
            LootFunction::SetCount {
                count,
                add,
                conditions,
            } => {
                if all_conditions_pass(conditions, ctx, random) {
                    let n = count.get_int(random);
                    if *add {
                        stack.count = stack.count.wrapping_add(n);
                    } else {
                        stack.count = n;
                    }
                }
            }
            LootFunction::ApplyBonus {
                enchantment,
                formula,
                conditions,
            } => {
                if all_conditions_pass(conditions, ctx, random) {
                    let level = ctx.tool.enchantment_level(enchantment).max(0);
                    stack.count = match *formula {
                        BonusFormula::OreDrops => apply_bonus_ore_drops(random, stack.count, level),
                        BonusFormula::UniformBonusCount { bonus_multiplier } => {
                            apply_bonus_uniform(random, stack.count, level, bonus_multiplier)
                        }
                        BonusFormula::BinomialWithBonusCount { extra, probability } => {
                            apply_bonus_binomial(random, stack.count, level, extra, probability)
                        }
                    };
                }
            }
            LootFunction::LimitCount {
                min,
                max,
                conditions,
            } => {
                if all_conditions_pass(conditions, ctx, random) {
                    if let Some(lo) = min {
                        stack.count = stack.count.max(*lo);
                    }
                    if let Some(hi) = max {
                        stack.count = stack.count.min(*hi);
                    }
                }
            }
            LootFunction::ExplosionDecay { conditions } => {
                if all_conditions_pass(conditions, ctx, random) {
                    stack.count = apply_explosion_decay(random, stack.count, ctx.explosion_radius);
                }
            }
            LootFunction::NoOp => {}
        }
    }
}

// ---------------------------------------------------------------------------
// JSON Parsing Helpers
// ---------------------------------------------------------------------------

fn parse_pool(value: &Value) -> Result<LootPool, LootTableError> {
    let obj = value
        .as_object()
        .ok_or_else(|| LootTableError::Schema("pool must be an object".into()))?;
    let rolls = obj
        .get("rolls")
        .map(parse_number_provider)
        .transpose()?
        .unwrap_or(NumberProvider::Constant(1.0));
    let bonus_rolls = obj
        .get("bonus_rolls")
        .map(parse_number_provider)
        .transpose()?
        .unwrap_or(NumberProvider::Constant(0.0));
    let conditions = parse_conditions(obj.get("conditions"))?;
    let functions = parse_functions(obj.get("functions"))?;

    let mut entries = Vec::new();
    if let Some(arr) = obj.get("entries").and_then(Value::as_array) {
        for e in arr {
            entries.push(parse_entry(e)?);
        }
    }
    Ok(LootPool {
        rolls,
        bonus_rolls,
        conditions,
        entries,
        functions,
    })
}

fn parse_entry(value: &Value) -> Result<LootEntry, LootTableError> {
    let obj = value
        .as_object()
        .ok_or_else(|| LootTableError::Schema("entry must be an object".into()))?;
    let kind = obj
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| LootTableError::Schema("entry missing 'type'".into()))?;
    let conditions = parse_conditions(obj.get("conditions"))?;

    match strip_minecraft_ns(kind) {
        "item" => {
            let name = obj
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| LootTableError::Schema("item entry missing 'name'".into()))?;
            let functions = parse_functions(obj.get("functions"))?;
            Ok(LootEntry::Item {
                name: normalize_id(name),
                conditions,
                functions,
            })
        }
        "alternatives" => {
            let children = parse_children(obj.get("children"))?;
            Ok(LootEntry::Alternatives {
                children,
                conditions,
            })
        }
        "group" => {
            let children = parse_children(obj.get("children"))?;
            Ok(LootEntry::Group {
                children,
                conditions,
            })
        }
        "sequence" => {
            let children = parse_children(obj.get("children"))?;
            Ok(LootEntry::Sequence {
                children,
                conditions,
            })
        }
        "empty" | "dynamic" => Ok(LootEntry::Empty { conditions }),
        other => Err(LootTableError::Schema(format!(
            "unsupported loot entry type '{other}'"
        ))),
    }
}

fn parse_children(value: Option<&Value>) -> Result<Vec<LootEntry>, LootTableError> {
    let mut out = Vec::new();
    if let Some(arr) = value.and_then(Value::as_array) {
        for child in arr {
            out.push(parse_entry(child)?);
        }
    }
    Ok(out)
}

fn parse_conditions(value: Option<&Value>) -> Result<Vec<LootCondition>, LootTableError> {
    let mut out = Vec::new();
    if let Some(arr) = value.and_then(Value::as_array) {
        for c in arr {
            out.push(parse_condition(c)?);
        }
    }
    Ok(out)
}

fn parse_condition(value: &Value) -> Result<LootCondition, LootTableError> {
    let obj = value
        .as_object()
        .ok_or_else(|| LootTableError::Schema("condition must be an object".into()))?;
    let kind = obj
        .get("condition")
        .and_then(Value::as_str)
        .ok_or_else(|| LootTableError::Schema("condition missing 'condition'".into()))?;

    match strip_minecraft_ns(kind) {
        "survives_explosion" => Ok(LootCondition::SurvivesExplosion),
        "block_state_property" => {
            let block = obj
                .get("block")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    LootTableError::Schema("block_state_property missing 'block'".into())
                })?
                .to_string();
            let mut properties = IndexMap::new();
            if let Some(props) = obj.get("properties").and_then(Value::as_object) {
                for (k, v) in props {
                    let s = match v {
                        Value::String(s) => s.clone(),
                        Value::Bool(b) => b.to_string(),
                        Value::Number(n) => n.to_string(),
                        _ => continue,
                    };
                    properties.insert(k.clone(), s);
                }
            }
            Ok(LootCondition::BlockStateProperty { block, properties })
        }
        "match_tool" => parse_match_tool(obj.get("predicate")),
        "any_of" => {
            let terms = parse_conditions(obj.get("terms"))?;
            Ok(LootCondition::AnyOf(terms))
        }
        "all_of" => {
            let terms = parse_conditions(obj.get("terms"))?;
            Ok(LootCondition::AllOf(terms))
        }
        "inverted" => {
            let term = obj
                .get("term")
                .ok_or_else(|| LootTableError::Schema("inverted missing 'term'".into()))?;
            Ok(LootCondition::Inverted(Box::new(parse_condition(term)?)))
        }
        "table_bonus" => {
            let enchantment = obj
                .get("enchantment")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    LootTableError::Schema("table_bonus missing 'enchantment'".into())
                })?;
            let chances = obj
                .get("chances")
                .and_then(Value::as_array)
                .ok_or_else(|| LootTableError::Schema("table_bonus missing 'chances'".into()))?
                .iter()
                .filter_map(|v| v.as_f64().map(|f| f as f32))
                .collect();
            Ok(LootCondition::TableBonus {
                enchantment: normalize_id(enchantment),
                chances,
            })
        }
        "random_chance" => {
            let chance =
                obj.get("chance").and_then(Value::as_f64).ok_or_else(|| {
                    LootTableError::Schema("random_chance missing 'chance'".into())
                })? as f32;
            Ok(LootCondition::RandomChance(chance))
        }
        other => Ok(LootCondition::Unsupported(other.to_string())),
    }
}

fn parse_match_tool(predicate: Option<&Value>) -> Result<LootCondition, LootTableError> {
    let Some(pred) = predicate.and_then(Value::as_object) else {
        return Ok(LootCondition::MatchTool {
            items: None,
            enchantments: Vec::new(),
        });
    };

    let items = match pred.get("items") {
        Some(Value::String(s)) => Some(vec![s.clone()]),
        Some(Value::Array(arr)) => Some(
            arr.iter()
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned)
                .collect(),
        ),
        _ => None,
    };

    let mut enchantments = Vec::new();
    let ench_array = pred
        .get("predicates")
        .and_then(Value::as_object)
        .and_then(|p| p.get("minecraft:enchantments"))
        .and_then(Value::as_array);
    if let Some(arr) = ench_array {
        for item in arr {
            let Some(eobj) = item.as_object() else {
                continue;
            };
            let Some(ench_id) = eobj.get("enchantments").and_then(Value::as_str) else {
                continue;
            };
            let (min_level, max_level) = match eobj.get("levels") {
                Some(Value::Number(n)) => {
                    let v = n.as_i64().map(|x| x as i32);
                    (v, v)
                }
                Some(Value::Object(bounds)) => (
                    bounds.get("min").and_then(Value::as_i64).map(|x| x as i32),
                    bounds.get("max").and_then(Value::as_i64).map(|x| x as i32),
                ),
                _ => (Some(1), None),
            };
            enchantments.push(EnchantmentPredicate {
                enchantment: normalize_id(ench_id),
                min_level,
                max_level,
            });
        }
    }

    Ok(LootCondition::MatchTool {
        items,
        enchantments,
    })
}

fn parse_functions(value: Option<&Value>) -> Result<Vec<LootFunction>, LootTableError> {
    let mut out = Vec::new();
    if let Some(arr) = value.and_then(Value::as_array) {
        for f in arr {
            out.push(parse_function(f)?);
        }
    }
    Ok(out)
}

fn parse_function(value: &Value) -> Result<LootFunction, LootTableError> {
    let obj = value
        .as_object()
        .ok_or_else(|| LootTableError::Schema("function must be an object".into()))?;
    let kind = obj
        .get("function")
        .and_then(Value::as_str)
        .ok_or_else(|| LootTableError::Schema("function missing 'function'".into()))?;
    let conditions = parse_conditions(obj.get("conditions"))?;

    match strip_minecraft_ns(kind) {
        "set_count" => {
            let count_val = obj
                .get("count")
                .ok_or_else(|| LootTableError::Schema("set_count missing 'count'".into()))?;
            let count = parse_number_provider(count_val)?;
            let add = obj.get("add").and_then(Value::as_bool).unwrap_or(false);
            Ok(LootFunction::SetCount {
                count,
                add,
                conditions,
            })
        }
        "apply_bonus" => {
            let enchantment = obj
                .get("enchantment")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    LootTableError::Schema("apply_bonus missing 'enchantment'".into())
                })?;
            let formula_name = obj
                .get("formula")
                .and_then(Value::as_str)
                .ok_or_else(|| LootTableError::Schema("apply_bonus missing 'formula'".into()))?;
            let params = obj.get("parameters").and_then(Value::as_object);
            let formula = match strip_minecraft_ns(formula_name) {
                "ore_drops" => BonusFormula::OreDrops,
                "uniform_bonus_count" => {
                    let bonus_multiplier = params
                        .and_then(|p| p.get("bonusMultiplier"))
                        .and_then(Value::as_i64)
                        .unwrap_or(1) as i32;
                    BonusFormula::UniformBonusCount { bonus_multiplier }
                }
                "binomial_with_bonus_count" => {
                    let extra = params
                        .and_then(|p| p.get("extra"))
                        .and_then(Value::as_i64)
                        .unwrap_or(0) as i32;
                    let probability = params
                        .and_then(|p| p.get("probability"))
                        .and_then(Value::as_f64)
                        .unwrap_or(0.0) as f32;
                    BonusFormula::BinomialWithBonusCount { extra, probability }
                }
                other => {
                    return Err(LootTableError::Schema(format!(
                        "unsupported apply_bonus formula '{other}'"
                    )));
                }
            };
            Ok(LootFunction::ApplyBonus {
                enchantment: normalize_id(enchantment),
                formula,
                conditions,
            })
        }
        "limit_count" => {
            let limit = obj.get("limit").and_then(Value::as_object);
            let min = limit
                .and_then(|l| l.get("min"))
                .and_then(Value::as_f64)
                .map(|v| v as i32);
            let max = limit
                .and_then(|l| l.get("max"))
                .and_then(Value::as_f64)
                .map(|v| v as i32);
            Ok(LootFunction::LimitCount {
                min,
                max,
                conditions,
            })
        }
        "explosion_decay" => Ok(LootFunction::ExplosionDecay { conditions }),
        "copy_components" | "copy_state" => Ok(LootFunction::NoOp),
        other => Err(LootTableError::Schema(format!(
            "unsupported loot function '{other}'"
        ))),
    }
}

fn parse_number_provider(value: &Value) -> Result<NumberProvider, LootTableError> {
    if let Some(n) = value.as_f64() {
        return Ok(NumberProvider::Constant(n as f32));
    }
    let obj = value
        .as_object()
        .ok_or_else(|| LootTableError::Schema("number provider must be number or object".into()))?;
    let kind = obj
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("minecraft:uniform");
    match strip_minecraft_ns(kind) {
        "constant" => {
            let v = obj
                .get("value")
                .and_then(Value::as_f64)
                .ok_or_else(|| LootTableError::Schema("constant missing 'value'".into()))?
                as f32;
            Ok(NumberProvider::Constant(v))
        }
        "uniform" => {
            let min = obj
                .get("min")
                .and_then(Value::as_f64)
                .ok_or_else(|| LootTableError::Schema("uniform missing 'min'".into()))?
                as f32;
            let max = obj
                .get("max")
                .and_then(Value::as_f64)
                .ok_or_else(|| LootTableError::Schema("uniform missing 'max'".into()))?
                as f32;
            Ok(NumberProvider::Uniform { min, max })
        }
        "binomial" => {
            let n = obj
                .get("n")
                .and_then(Value::as_i64)
                .ok_or_else(|| LootTableError::Schema("binomial missing 'n'".into()))?
                as i32;
            let p = obj
                .get("p")
                .and_then(Value::as_f64)
                .ok_or_else(|| LootTableError::Schema("binomial missing 'p'".into()))?
                as f32;
            Ok(NumberProvider::Binomial { n, p })
        }
        other => Err(LootTableError::Schema(format!(
            "unsupported number provider '{other}'"
        ))),
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

#[inline]
fn strip_minecraft_ns(id: &str) -> &str {
    id.strip_prefix("minecraft:").unwrap_or(id)
}

#[inline]
fn normalize_id(id: &str) -> String {
    if id.contains(':') {
        id.to_string()
    } else {
        format!("minecraft:{id}")
    }
}

#[inline]
fn block_name_matches(block_name: &str, expected: &str) -> bool {
    strip_minecraft_ns(block_name) == strip_minecraft_ns(expected)
}

fn matches_item_tag(tool: &ToolStack, tag: &str) -> bool {
    match strip_minecraft_ns(tag) {
        "pickaxes" => tool.is_pickaxe(),
        "shears" => tool.is_shears(),
        _ => false,
    }
}

fn block_state_property_equals(state: BlockState, prop_name: &str, expected: &str) -> bool {
    let def = StateDefinition::for_block(state.block());
    let Some(prop) = def.get_property(prop_name) else {
        return false;
    };
    let Some(val) = state.get_value(prop) else {
        return false;
    };
    match val {
        PropertyValue::Bool(b) => (b && expected == "true") || (!b && expected == "false"),
        PropertyValue::Int(i) => expected.parse::<i32>().ok() == Some(i),
        PropertyValue::Enum(s) => s == expected,
    }
}

fn block_state_int_property(state: BlockState, prop_name: &str) -> Option<i32> {
    let def = StateDefinition::for_block(state.block());
    let prop = def.get_property(prop_name)?;
    match state.get_value(prop)? {
        PropertyValue::Int(i) => Some(i),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_registry::generated::blocks::BlockId;
    use rivet_util::random::SingleThreadedRandomSource;

    fn state_of(name: &str) -> BlockState {
        let id = BlockId::from_name(name)
            .unwrap_or_else(|| panic!("unknown block '{name}' in generated registry"));
        BlockState::of(id)
    }

    fn state_with_prop(name: &str, prop_name: &str, val: PropertyValue) -> BlockState {
        let base = state_of(name);
        let def = StateDefinition::for_block(base.block());
        let prop = def
            .get_property(prop_name)
            .unwrap_or_else(|| panic!("missing property '{prop_name}' on '{name}'"));
        base.set_value(prop, val)
            .unwrap_or_else(|e| panic!("failed to set {prop_name}={val:?} on {name}: {e}"))
    }

    #[test]
    fn creative_mode_prevents_block_drops_and_xp_without_consuming_rng() {
        let mut rng_a = SingleThreadedRandomSource::new(12345);
        let mut rng_b = SingleThreadedRandomSource::new(12345);
        let ctx = BlockBreakContext::for_player(
            GameType::Creative,
            state_of("minecraft:diamond_ore"),
            BlockPos::new(0, 64, 0),
            ToolStack::tool(ToolTier::Diamond, ToolKind::Pickaxe),
        );
        let res = resolve_block_break(&ctx, &mut rng_a);
        assert!(res.prevents_block_drops);
        assert!(res.items.is_empty());
        assert_eq!(res.experience, 0);
        // RNG untouched by Creative break.
        assert_eq!(rng_a.next_int(), rng_b.next_int());
    }

    #[test]
    fn block_drops_game_rule_false_suppresses_items_and_xp() {
        let mut rng = SingleThreadedRandomSource::new(42);
        let mut ctx = BlockBreakContext::for_player(
            GameType::Survival,
            state_of("minecraft:diamond_ore"),
            BlockPos::new(0, 64, 0),
            ToolStack::tool(ToolTier::Iron, ToolKind::Pickaxe),
        );
        ctx.block_drops_rule = false;
        let res = resolve_block_break(&ctx, &mut rng);
        assert!(res.has_correct_tool_for_drops);
        assert!(res.items.is_empty());
        assert_eq!(res.experience, 0);
    }

    #[test]
    fn stone_requires_pickaxe_and_respects_silk_touch() {
        let mut rng = SingleThreadedRandomSource::new(7);

        // Bare hand -> no drops.
        let bare = BlockBreakContext::for_player(
            GameType::Survival,
            state_of("minecraft:stone"),
            BlockPos::new(1, 64, 1),
            ToolStack::empty(),
        );
        let res_bare = resolve_block_break(&bare, &mut rng);
        assert!(!res_bare.has_correct_tool_for_drops);
        assert!(res_bare.items.is_empty());

        // Wooden pickaxe -> cobblestone.
        let wood_pick = BlockBreakContext::for_player(
            GameType::Survival,
            state_of("minecraft:stone"),
            BlockPos::new(1, 64, 1),
            ToolStack::tool(ToolTier::Wood, ToolKind::Pickaxe),
        );
        let res_wood = resolve_block_break(&wood_pick, &mut rng);
        assert!(res_wood.has_correct_tool_for_drops);
        assert_eq!(
            res_wood.items,
            vec![BlockDropStack::new("minecraft:cobblestone", 1)]
        );
        assert_eq!(res_wood.experience, 0);

        // Silk Touch wooden pickaxe -> stone.
        let silk_pick = BlockBreakContext::for_player(
            GameType::Survival,
            state_of("minecraft:stone"),
            BlockPos::new(1, 64, 1),
            ToolStack::tool(ToolTier::Wood, ToolKind::Pickaxe)
                .with_enchantment("minecraft:silk_touch", 1),
        );
        let res_silk = resolve_block_break(&silk_pick, &mut rng);
        assert_eq!(
            res_silk.items,
            vec![BlockDropStack::new("minecraft:stone", 1)]
        );
    }

    #[test]
    fn tool_tier_gating_matches_paper_incorrect_for_tool_tags() {
        let mut rng = SingleThreadedRandomSource::new(99);
        let pos = BlockPos::new(0, 10, 0);

        // Iron ore: wood denied, stone/copper/iron allowed.
        let iron_ore = state_of("minecraft:iron_ore");
        assert!(!has_correct_tool_for_drops(
            iron_ore,
            &ToolStack::tool(ToolTier::Wood, ToolKind::Pickaxe)
        ));
        assert!(has_correct_tool_for_drops(
            iron_ore,
            &ToolStack::tool(ToolTier::Stone, ToolKind::Pickaxe)
        ));
        assert!(has_correct_tool_for_drops(
            iron_ore,
            &ToolStack::tool(ToolTier::Copper, ToolKind::Pickaxe)
        ));

        // Diamond ore: stone denied, iron allowed.
        let diamond_ore = state_of("minecraft:diamond_ore");
        assert!(!has_correct_tool_for_drops(
            diamond_ore,
            &ToolStack::tool(ToolTier::Stone, ToolKind::Pickaxe)
        ));
        let res_stone = resolve_block_break(
            &BlockBreakContext::for_player(
                GameType::Survival,
                diamond_ore,
                pos,
                ToolStack::tool(ToolTier::Stone, ToolKind::Pickaxe),
            ),
            &mut rng,
        );
        assert!(res_stone.items.is_empty());
        assert_eq!(res_stone.experience, 0);

        let res_iron = resolve_block_break(
            &BlockBreakContext::for_player(
                GameType::Survival,
                diamond_ore,
                pos,
                ToolStack::tool(ToolTier::Iron, ToolKind::Pickaxe),
            ),
            &mut rng,
        );
        assert_eq!(
            res_iron.items,
            vec![BlockDropStack::new("minecraft:diamond", 1)]
        );
        assert!((3..=7).contains(&res_iron.experience));

        // Obsidian: iron denied, diamond & netherite allowed.
        let obsidian = state_of("minecraft:obsidian");
        assert!(!has_correct_tool_for_drops(
            obsidian,
            &ToolStack::tool(ToolTier::Iron, ToolKind::Pickaxe)
        ));
        assert!(has_correct_tool_for_drops(
            obsidian,
            &ToolStack::tool(ToolTier::Diamond, ToolKind::Pickaxe)
        ));
        assert!(has_correct_tool_for_drops(
            obsidian,
            &ToolStack::tool(ToolTier::Netherite, ToolKind::Pickaxe)
        ));
    }

    #[test]
    fn bare_hand_ordinary_blocks_drop_faithfully() {
        let mut rng = SingleThreadedRandomSource::new(11);
        let pos = BlockPos::new(0, 64, 0);

        // Dirt -> dirt.
        let dirt = resolve_block_break(
            &BlockBreakContext::for_player(
                GameType::Survival,
                state_of("minecraft:dirt"),
                pos,
                ToolStack::empty(),
            ),
            &mut rng,
        );
        assert_eq!(dirt.items, vec![BlockDropStack::new("minecraft:dirt", 1)]);

        // Grass block -> dirt without Silk Touch, grass_block with Silk Touch.
        let grass = resolve_block_break(
            &BlockBreakContext::for_player(
                GameType::Survival,
                state_of("minecraft:grass_block"),
                pos,
                ToolStack::empty(),
            ),
            &mut rng,
        );
        assert_eq!(grass.items, vec![BlockDropStack::new("minecraft:dirt", 1)]);

        let grass_silk = resolve_block_break(
            &BlockBreakContext::for_player(
                GameType::Survival,
                state_of("minecraft:grass_block"),
                pos,
                ToolStack::tool(ToolTier::Iron, ToolKind::Shovel)
                    .with_enchantment("minecraft:silk_touch", 1),
            ),
            &mut rng,
        );
        assert_eq!(
            grass_silk.items,
            vec![BlockDropStack::new("minecraft:grass_block", 1)]
        );

        // Oak log -> oak_log.
        let log = resolve_block_break(
            &BlockBreakContext::for_player(
                GameType::Survival,
                state_of("minecraft:oak_log"),
                pos,
                ToolStack::empty(),
            ),
            &mut rng,
        );
        assert_eq!(log.items, vec![BlockDropStack::new("minecraft:oak_log", 1)]);

        // Glass -> nothing without Silk Touch, glass with Silk Touch.
        let glass = resolve_block_break(
            &BlockBreakContext::for_player(
                GameType::Survival,
                state_of("minecraft:glass"),
                pos,
                ToolStack::empty(),
            ),
            &mut rng,
        );
        assert!(glass.items.is_empty());
        let glass_silk = resolve_block_break(
            &BlockBreakContext::for_player(
                GameType::Survival,
                state_of("minecraft:glass"),
                pos,
                ToolStack::item("minecraft:stick").with_enchantment("minecraft:silk_touch", 1),
            ),
            &mut rng,
        );
        assert_eq!(
            glass_silk.items,
            vec![BlockDropStack::new("minecraft:glass", 1)]
        );
    }

    #[test]
    fn fortune_three_gravel_always_drops_flint_and_silk_touch_drops_gravel() {
        let mut rng = SingleThreadedRandomSource::new(2026);
        let pos = BlockPos::new(0, 64, 0);

        let fortune3 = ToolStack::tool(ToolTier::Diamond, ToolKind::Shovel)
            .with_enchantment("minecraft:fortune", 3);
        for _ in 0..32 {
            let res = resolve_block_break(
                &BlockBreakContext::for_player(
                    GameType::Survival,
                    state_of("minecraft:gravel"),
                    pos,
                    fortune3.clone(),
                ),
                &mut rng,
            );
            assert_eq!(res.items, vec![BlockDropStack::new("minecraft:flint", 1)]);
        }

        let silk = ToolStack::tool(ToolTier::Diamond, ToolKind::Shovel)
            .with_enchantment("minecraft:silk_touch", 1);
        let res_silk = resolve_block_break(
            &BlockBreakContext::for_player(
                GameType::Survival,
                state_of("minecraft:gravel"),
                pos,
                silk,
            ),
            &mut rng,
        );
        assert_eq!(
            res_silk.items,
            vec![BlockDropStack::new("minecraft:gravel", 1)]
        );
    }

    #[test]
    fn fortune_ore_drops_and_glowstone_clamping_match_paper_ranges() {
        let mut rng = SingleThreadedRandomSource::new(777);
        let pos = BlockPos::new(0, 64, 0);
        let fortune3 = ToolStack::tool(ToolTier::Diamond, ToolKind::Pickaxe)
            .with_enchantment("minecraft:fortune", 3);

        // Diamond ore with Fortune III: 1..=4 diamonds, 3..=7 XP.
        let mut saw_multi = false;
        for _ in 0..64 {
            let res = resolve_block_break(
                &BlockBreakContext::for_player(
                    GameType::Survival,
                    state_of("minecraft:diamond_ore"),
                    pos,
                    fortune3.clone(),
                ),
                &mut rng,
            );
            assert_eq!(res.items.len(), 1);
            assert_eq!(res.items[0].item_id, "minecraft:diamond");
            assert!((1..=4).contains(&res.items[0].count));
            if res.items[0].count > 1 {
                saw_multi = true;
            }
            assert!((3..=7).contains(&res.experience));
        }
        assert!(saw_multi, "Fortune III should occasionally multiply drops");

        // Glowstone with Fortune III is clamped to 1..=4 per Paper limit_count.
        for _ in 0..64 {
            let res = resolve_block_break(
                &BlockBreakContext::for_player(
                    GameType::Survival,
                    state_of("minecraft:glowstone"),
                    pos,
                    fortune3.clone(),
                ),
                &mut rng,
            );
            assert_eq!(res.items.len(), 1);
            assert_eq!(res.items[0].item_id, "minecraft:glowstone_dust");
            assert!((1..=4).contains(&res.items[0].count));
        }
    }

    #[test]
    fn silk_touch_suppresses_ore_xp_but_not_spawner_xp() {
        let mut rng = SingleThreadedRandomSource::new(55);
        let silk_pick = ToolStack::tool(ToolTier::Diamond, ToolKind::Pickaxe)
            .with_enchantment("minecraft:silk_touch", 1);

        let coal = resolve_block_break(
            &BlockBreakContext::for_player(
                GameType::Survival,
                state_of("minecraft:coal_ore"),
                BlockPos::new(0, 50, 0),
                silk_pick.clone(),
            ),
            &mut rng,
        );
        assert_eq!(
            coal.items,
            vec![BlockDropStack::new("minecraft:coal_ore", 1)]
        );
        assert_eq!(coal.experience, 0);

        let spawner = resolve_block_break(
            &BlockBreakContext::for_player(
                GameType::Survival,
                state_of("minecraft:spawner"),
                BlockPos::new(0, 50, 0),
                silk_pick,
            ),
            &mut rng,
        );
        assert!(spawner.items.is_empty());
        assert!((15..=43).contains(&spawner.experience));
    }

    #[test]
    fn cobweb_and_leaves_respect_shears_and_sword() {
        let mut rng = SingleThreadedRandomSource::new(33);
        let pos = BlockPos::new(0, 64, 0);

        // Cobweb: bare hand denied, sword drops string, shears drops cobweb.
        let bare = resolve_block_break(
            &BlockBreakContext::for_player(
                GameType::Survival,
                state_of("minecraft:cobweb"),
                pos,
                ToolStack::empty(),
            ),
            &mut rng,
        );
        assert!(!bare.has_correct_tool_for_drops);
        assert!(bare.items.is_empty());

        let sword = resolve_block_break(
            &BlockBreakContext::for_player(
                GameType::Survival,
                state_of("minecraft:cobweb"),
                pos,
                ToolStack::sword(ToolTier::Iron),
            ),
            &mut rng,
        );
        assert!(sword.has_correct_tool_for_drops);
        assert_eq!(
            sword.items,
            vec![BlockDropStack::new("minecraft:string", 1)]
        );

        let shears = resolve_block_break(
            &BlockBreakContext::for_player(
                GameType::Survival,
                state_of("minecraft:cobweb"),
                pos,
                ToolStack::shears(),
            ),
            &mut rng,
        );
        assert_eq!(
            shears.items,
            vec![BlockDropStack::new("minecraft:cobweb", 1)]
        );

        // Oak leaves with shears -> oak_leaves.
        let leaves = resolve_block_break(
            &BlockBreakContext::for_player(
                GameType::Survival,
                state_of("minecraft:oak_leaves"),
                pos,
                ToolStack::shears(),
            ),
            &mut rng,
        );
        assert_eq!(
            leaves.items,
            vec![BlockDropStack::new("minecraft:oak_leaves", 1)]
        );
    }

    #[test]
    fn block_state_properties_control_slab_snow_and_wheat_drops() {
        let mut rng = SingleThreadedRandomSource::new(101);
        let pos = BlockPos::new(0, 64, 0);

        // Double stone slab -> 2 stone slabs with pickaxe.
        let double_slab = state_with_prop(
            "minecraft:stone_slab",
            "type",
            PropertyValue::Enum("double"),
        );
        let res_slab = resolve_block_break(
            &BlockBreakContext::for_player(
                GameType::Survival,
                double_slab,
                pos,
                ToolStack::tool(ToolTier::Stone, ToolKind::Pickaxe),
            ),
            &mut rng,
        );
        assert_eq!(
            res_slab.items,
            vec![BlockDropStack::new("minecraft:stone_slab", 2)]
        );

        // Snow layers=3: bare hand denied, shovel drops 3 snowballs.
        let snow3 = state_with_prop("minecraft:snow", "layers", PropertyValue::Int(3));
        let res_snow_hand = resolve_block_break(
            &BlockBreakContext::for_player(GameType::Survival, snow3, pos, ToolStack::empty()),
            &mut rng,
        );
        assert!(!res_snow_hand.has_correct_tool_for_drops);
        assert!(res_snow_hand.items.is_empty());

        let res_snow_shovel = resolve_block_break(
            &BlockBreakContext::for_player(
                GameType::Survival,
                snow3,
                pos,
                ToolStack::tool(ToolTier::Wood, ToolKind::Shovel),
            ),
            &mut rng,
        );
        assert_eq!(
            res_snow_shovel.items,
            vec![BlockDropStack::new("minecraft:snowball", 3)]
        );

        // Mature wheat (age=7) drops 1 wheat + 1..=4 seeds; immature (age=3) drops 1 seed.
        let mature_wheat = state_with_prop("minecraft:wheat", "age", PropertyValue::Int(7));
        let res_mature = resolve_block_break(
            &BlockBreakContext::for_player(
                GameType::Survival,
                mature_wheat,
                pos,
                ToolStack::empty(),
            ),
            &mut rng,
        );
        assert_eq!(
            res_mature.items[0],
            BlockDropStack::new("minecraft:wheat", 1)
        );
        assert_eq!(res_mature.items[1].item_id, "minecraft:wheat_seeds");
        assert!((1..=4).contains(&res_mature.items[1].count));

        let immature_wheat = state_with_prop("minecraft:wheat", "age", PropertyValue::Int(3));
        let res_immature = resolve_block_break(
            &BlockBreakContext::for_player(
                GameType::Survival,
                immature_wheat,
                pos,
                ToolStack::empty(),
            ),
            &mut rng,
        );
        assert_eq!(
            res_immature.items,
            vec![BlockDropStack::new("minecraft:wheat_seeds", 1)]
        );
    }

    #[test]
    fn json_loot_table_evaluator_matches_paper_26_2_glowstone_and_stone_tables() {
        let glowstone_json = r#"{
          "type": "minecraft:block",
          "pools": [
            {
              "entries": [
                {
                  "type": "minecraft:alternatives",
                  "children": [
                    {
                      "type": "minecraft:item",
                      "conditions": [
                        {
                          "condition": "minecraft:match_tool",
                          "predicate": {
                            "predicates": {
                              "minecraft:enchantments": [
                                {
                                  "enchantments": "minecraft:silk_touch",
                                  "levels": { "min": 1 }
                                }
                              ]
                            }
                          }
                        }
                      ],
                      "name": "minecraft:glowstone"
                    },
                    {
                      "type": "minecraft:item",
                      "functions": [
                        {
                          "count": { "type": "minecraft:uniform", "max": 4.0, "min": 2.0 },
                          "function": "minecraft:set_count"
                        },
                        {
                          "enchantment": "minecraft:fortune",
                          "formula": "minecraft:uniform_bonus_count",
                          "function": "minecraft:apply_bonus",
                          "parameters": { "bonusMultiplier": 1 }
                        },
                        {
                          "function": "minecraft:limit_count",
                          "limit": { "max": 4.0, "min": 1.0 }
                        },
                        { "function": "minecraft:explosion_decay" }
                      ],
                      "name": "minecraft:glowstone_dust"
                    }
                  ]
                }
              ],
              "rolls": 1.0
            }
          ]
        }"#;

        let table = BlockLootTable::from_json(glowstone_json).expect("valid glowstone table");
        let pos = BlockPos::new(0, 64, 0);

        // Compare JSON table evaluation bit-for-bit with built-in resolver using identical RNG seeds.
        for seed in [1_i64, 42, 999, 123456] {
            let mut rng_json = SingleThreadedRandomSource::new(seed);
            let mut rng_builtin = SingleThreadedRandomSource::new(seed);
            let tool = ToolStack::tool(ToolTier::Diamond, ToolKind::Pickaxe)
                .with_enchantment("minecraft:fortune", 2);
            let ctx = BlockBreakContext::for_player(
                GameType::Survival,
                state_of("minecraft:glowstone"),
                pos,
                tool,
            );
            let from_json = resolve_block_break_with_table(&ctx, Some(&table), &mut rng_json);
            let from_builtin = resolve_block_break(&ctx, &mut rng_builtin);
            assert_eq!(from_json, from_builtin);
        }

        // Silk Touch via JSON table returns glowstone.
        let mut rng = SingleThreadedRandomSource::new(5);
        let silk_ctx = BlockBreakContext::for_player(
            GameType::Survival,
            state_of("minecraft:glowstone"),
            pos,
            ToolStack::empty().with_enchantment("minecraft:silk_touch", 1),
        );
        let silk_res = resolve_block_break_with_table(&silk_ctx, Some(&table), &mut rng);
        assert_eq!(
            silk_res.items,
            vec![BlockDropStack::new("minecraft:glowstone", 1)]
        );
    }

    #[test]
    fn explosion_decay_reduces_drops_probabilistically() {
        let mut rng = SingleThreadedRandomSource::new(2026);
        let ctx = BlockDropContext::new(
            state_of("minecraft:stone"),
            BlockPos::new(0, 64, 0),
            ToolStack::tool(ToolTier::Iron, ToolKind::Pickaxe),
        )
        .with_explosion_radius(4.0);

        let mut survived = 0;
        let trials = 200;
        for _ in 0..trials {
            if !resolve_builtin_block_drops(&ctx, &mut rng).is_empty() {
                survived += 1;
            }
        }
        // Expected ~25% (50 of 200); assert strictly between 0 and trials.
        assert!(survived > 15 && survived < 90, "survived={survived}");
    }
}
