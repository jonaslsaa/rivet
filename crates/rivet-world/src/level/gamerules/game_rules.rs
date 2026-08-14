//! `net.minecraft.world.level.gamerules.GameRules` — the 59 built-in game rules
//! and the GAME_RULE registry.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/level/
//! gamerules/GameRules.java`.
//!
//! Every static rule is a `registerBoolean`/`registerInteger` call in Java's
//! exact declaration order; the port folds them into the GAME_RULE registry
//! bootstrap (the `FeatureSizeTypeId` pattern). `ADVANCE_TIME` is the bootstrap
//! element (`GameRules.bootstrap`). `SharedConstants.DEBUG_WORLD_RECREATE` is
//! not ported (`rivet-core` has no such constant), so `!DEBUG_WORLD_RECREATE`
//! evaluates to `true` exactly as the shipping server's non-debug builds.
//!
//! `GameRules.availableRules`/`getAsString`/`copy`/`visitGameRuleTypes` are
//! deferred where their `ServerLevel` callback seam is unported — `set`/
//! `setAll`/`setFromOther` take a `@Nullable ServerLevel` in Java for
//! `level.getServer().onGameRuleChanged(...)`, and `ServerLevel` lives in
//! `rivet-server` (the seam is a `RivetTodo` below).

use crate::flag::feature_flag_set::FeatureFlagSet;
use crate::flag::feature_flags::MINECART_IMPROVEMENTS;
use crate::level::gamerules::game_rule::{
    ArgumentErased, GameRuleErased, GameRuleValue, GameRuleValueCodec, VisitorCaller,
};
use crate::level::gamerules::game_rule_category::GameRuleCategory;
use crate::level::gamerules::game_rule_map::{self, GameRuleMap};
use crate::level::gamerules::game_rule_type::GameRuleType;
use crate::level::gamerules::game_rule_type_visitor::GameRuleTypeVisitor;
use rivet_brigadier::arguments::bool_argument_type::BoolArgumentType;
use rivet_brigadier::arguments::integer_argument_type::IntegerArgumentType;
use rivet_registry::{
    Identifier, RegistrationInfo, Registry, RegistryAccess, RegistryBuilder, ResourceKey,
};
use std::sync::{Arc, LazyLock};

/// `BuiltInRegistries.GAME_RULE` — `registerSimple(Registries.GAME_RULE,
/// GameRules::bootstrap)`. The registry key and the built-in `RegistryAccess`
/// (the same pattern as `rivet-registry::feature_size_type`).
pub static GAME_RULE: LazyLock<ResourceKey<Registry<GameRuleErased>>> = LazyLock::new(|| {
    ResourceKey::create_registry_key(Identifier::with_default_namespace("game_rule"))
});

/// The built-in GAME_RULE `RegistryAccess` — the canonical registry instance.
/// `Registry` encodes values by allocation identity, so every accessor shares
/// this one instance (mirroring the `FeatureSizeTypeId::built_in_registry_access`
/// doc note).
static BUILT_IN_REGISTRY_ACCESS: LazyLock<RegistryAccess> = LazyLock::new(|| {
    let mut builder = RegistryBuilder::new(&GAME_RULE);
    for rule in BUILT_IN_RULES.iter() {
        let key = ResourceKey::create(&GAME_RULE, Identifier::parse(rule.0));
        builder.register(&key, rule.1.clone(), RegistrationInfo::BUILT_IN);
    }
    RegistryAccess::from_single_registry((*GAME_RULE).clone(), builder.freeze())
});

/// `BuiltInRegistries.GAME_RULE` — the frozen built-in registry.
pub fn built_in_registry() -> &'static Registry<GameRuleErased> {
    BUILT_IN_REGISTRY_ACCESS
        .lookup(&GAME_RULE)
        .expect("built-in GAME_RULE registry is present")
}

/// The 59 built-in rules in Java's exact declaration order — `(identifier,
/// rule)`. The `Arc` is the registry-held identity (`GameRuleIndex` = insertion
/// order). Rules declared with `GameRuleTypeVisitor::visitBoolean` /
/// `::visitInteger` dispatch the typed visitor method (their `VisitorCaller`).
static BUILT_IN_RULES: LazyLock<Vec<(&'static str, Arc<GameRuleErased>)>> = LazyLock::new(|| {
    vec![
        (
            "advance_time",
            register_boolean(GameRuleCategory::updates(), true),
        ),
        (
            "advance_weather",
            register_boolean(GameRuleCategory::updates(), true),
        ),
        (
            "allow_entering_nether_using_portals",
            register_boolean(GameRuleCategory::misc(), true),
        ),
        (
            "block_drops",
            register_boolean(GameRuleCategory::drops(), true),
        ),
        (
            "block_explosion_drop_decay",
            register_boolean(GameRuleCategory::drops(), true),
        ),
        (
            "command_blocks_work",
            register_boolean(GameRuleCategory::misc(), true),
        ),
        (
            "command_block_output",
            register_boolean(GameRuleCategory::chat(), true),
        ),
        (
            "drowning_damage",
            register_boolean(GameRuleCategory::player(), true),
        ),
        (
            "elytra_movement_check",
            register_boolean(GameRuleCategory::player(), true),
        ),
        (
            "ender_pearls_vanish_on_death",
            register_boolean(GameRuleCategory::player(), true),
        ),
        (
            "entity_drops",
            register_boolean(GameRuleCategory::drops(), true),
        ),
        (
            "fall_damage",
            register_boolean(GameRuleCategory::player(), true),
        ),
        (
            "fire_damage",
            register_boolean(GameRuleCategory::player(), true),
        ),
        (
            "fire_spread_radius_around_player",
            register_integer(
                GameRuleCategory::updates(),
                128,
                -1,
                i32::MAX,
                FeatureFlagSet::of(),
            ),
        ),
        (
            "forgive_dead_players",
            register_boolean(GameRuleCategory::mobs(), true),
        ),
        (
            "freeze_damage",
            register_boolean(GameRuleCategory::player(), true),
        ),
        (
            "global_sound_events",
            register_boolean(GameRuleCategory::misc(), true),
        ),
        (
            "immediate_respawn",
            register_boolean(GameRuleCategory::player(), false),
        ),
        (
            "keep_inventory",
            register_boolean(GameRuleCategory::player(), false),
        ),
        (
            "lava_source_conversion",
            register_boolean(GameRuleCategory::updates(), false),
        ),
        (
            "limited_crafting",
            register_boolean(GameRuleCategory::player(), false),
        ),
        (
            "locator_bar",
            register_boolean(GameRuleCategory::player(), true),
        ),
        (
            "log_admin_commands",
            register_boolean(GameRuleCategory::chat(), true),
        ),
        (
            "max_block_modifications",
            register_integer(
                GameRuleCategory::misc(),
                32768,
                1,
                i32::MAX,
                FeatureFlagSet::of(),
            ),
        ),
        (
            "max_command_forks",
            register_integer(
                GameRuleCategory::misc(),
                65536,
                0,
                i32::MAX,
                FeatureFlagSet::of(),
            ),
        ),
        (
            "max_command_sequence_length",
            register_integer(
                GameRuleCategory::misc(),
                65536,
                0,
                i32::MAX,
                FeatureFlagSet::of(),
            ),
        ),
        (
            "max_entity_cramming",
            register_integer(
                GameRuleCategory::mobs(),
                24,
                0,
                i32::MAX,
                FeatureFlagSet::of(),
            ),
        ),
        (
            "max_minecart_speed",
            register_integer(
                GameRuleCategory::misc(),
                8,
                1,
                1000,
                FeatureFlagSet::of_flag(&MINECART_IMPROVEMENTS),
            ),
        ),
        (
            "max_snow_accumulation_height",
            register_integer(GameRuleCategory::updates(), 1, 0, 8, FeatureFlagSet::of()),
        ),
        (
            "mob_drops",
            register_boolean(GameRuleCategory::drops(), true),
        ),
        (
            "mob_explosion_drop_decay",
            register_boolean(GameRuleCategory::drops(), true),
        ),
        (
            "mob_griefing",
            register_boolean(GameRuleCategory::mobs(), true),
        ),
        (
            "natural_health_regeneration",
            register_boolean(GameRuleCategory::player(), true),
        ),
        (
            "player_movement_check",
            register_boolean(GameRuleCategory::player(), true),
        ),
        (
            "players_nether_portal_creative_delay",
            register_integer(
                GameRuleCategory::player(),
                0,
                0,
                i32::MAX,
                FeatureFlagSet::of(),
            ),
        ),
        (
            "players_nether_portal_default_delay",
            register_integer(
                GameRuleCategory::player(),
                80,
                0,
                i32::MAX,
                FeatureFlagSet::of(),
            ),
        ),
        (
            "players_sleeping_percentage",
            register_integer(
                GameRuleCategory::player(),
                100,
                0,
                i32::MAX,
                FeatureFlagSet::of(),
            ),
        ),
        (
            "projectiles_can_break_blocks",
            register_boolean(GameRuleCategory::drops(), true),
        ),
        ("pvp", register_boolean(GameRuleCategory::player(), true)),
        ("raids", register_boolean(GameRuleCategory::mobs(), true)),
        (
            "random_tick_speed",
            register_integer(
                GameRuleCategory::updates(),
                3,
                0,
                i32::MAX,
                FeatureFlagSet::of(),
            ),
        ),
        (
            "reduced_debug_info",
            register_boolean(GameRuleCategory::misc(), false),
        ),
        (
            "respawn_radius",
            register_integer(
                GameRuleCategory::player(),
                10,
                0,
                i32::MAX,
                FeatureFlagSet::of(),
            ),
        ),
        (
            "send_command_feedback",
            register_boolean(GameRuleCategory::chat(), true),
        ),
        (
            "show_advancement_messages",
            register_boolean(GameRuleCategory::chat(), true),
        ),
        (
            "show_death_messages",
            register_boolean(GameRuleCategory::chat(), true),
        ),
        (
            "spawner_blocks_work",
            register_boolean(GameRuleCategory::misc(), true),
        ),
        (
            "spawn_mobs",
            register_boolean(GameRuleCategory::spawning(), true),
        ),
        (
            "spawn_monsters",
            register_boolean(GameRuleCategory::spawning(), true),
        ),
        (
            "spawn_patrols",
            register_boolean(GameRuleCategory::spawning(), true),
        ),
        (
            "spawn_phantoms",
            register_boolean(GameRuleCategory::spawning(), true),
        ),
        (
            "spawn_wandering_traders",
            register_boolean(GameRuleCategory::spawning(), true),
        ),
        (
            "spawn_wardens",
            register_boolean(GameRuleCategory::spawning(), true),
        ),
        (
            "spectators_generate_chunks",
            register_boolean(GameRuleCategory::player(), true),
        ),
        (
            "spread_vines",
            register_boolean(GameRuleCategory::updates(), true),
        ),
        (
            "tnt_explodes",
            register_boolean(GameRuleCategory::misc(), true),
        ),
        (
            "tnt_explosion_drop_decay",
            register_boolean(GameRuleCategory::drops(), false),
        ),
        (
            "universal_anger",
            register_boolean(GameRuleCategory::mobs(), false),
        ),
        (
            "water_source_conversion",
            register_boolean(GameRuleCategory::updates(), true),
        ),
    ]
});

/// `GameRules.registerBoolean(String, GameRuleCategory, boolean)` —
/// `GameRuleType.BOOL`, `BoolArgumentType.bool()`, `Codec.BOOL`,
/// `GameRuleTypeVisitor::visitBoolean`, `b -> b ? 1 : 0`.
fn register_boolean(category: &GameRuleCategory, default_value: bool) -> Arc<GameRuleErased> {
    Arc::new(GameRuleErased::new(
        category.clone(),
        GameRuleType::Bool,
        ArgumentErased::Bool(BoolArgumentType::bool()),
        visitor_caller_boolean(),
        GameRuleValueCodec::Bool,
        Arc::new(|value: &GameRuleValue| match value {
            GameRuleValue::Bool(b) => {
                if *b {
                    1
                } else {
                    0
                }
            }
            GameRuleValue::Int(_) => panic!("boolean rule command result"),
        }),
        GameRuleValue::Bool(default_value),
        FeatureFlagSet::of(),
    ))
}

/// `GameRules.registerInteger(String, GameRuleCategory, int, int, int,
/// FeatureFlagSet)` — `GameRuleType.INT`,
/// `IntegerArgumentType.integer(min, max)`, `Codec.intRange(min, max)`,
/// `GameRuleTypeVisitor::visitInteger`, `i -> i`.
fn register_integer(
    category: &GameRuleCategory,
    default_value: i32,
    min: i32,
    max: i32,
    required_features: FeatureFlagSet,
) -> Arc<GameRuleErased> {
    Arc::new(GameRuleErased::new(
        category.clone(),
        GameRuleType::Int,
        ArgumentErased::Int(IntegerArgumentType::integer_with_bounds(min, max)),
        visitor_caller_integer(),
        GameRuleValueCodec::Int { min, max },
        Arc::new(|value: &GameRuleValue| match value {
            GameRuleValue::Int(i) => *i,
            GameRuleValue::Bool(_) => panic!("integer rule command result"),
        }),
        GameRuleValue::Int(default_value),
        required_features,
    ))
}

/// `GameRuleTypeVisitor::visitBoolean` — the `VisitorCaller<Boolean>`.
fn visitor_caller_boolean() -> VisitorCaller {
    Arc::new(
        |visitor: &mut dyn GameRuleTypeVisitor, key: &GameRuleErased| {
            visitor.visit_boolean(key);
        },
    )
}

/// `GameRuleTypeVisitor::visitInteger` — the `VisitorCaller<Integer>`.
fn visitor_caller_integer() -> VisitorCaller {
    Arc::new(
        |visitor: &mut dyn GameRuleTypeVisitor, key: &GameRuleErased| {
            visitor.visit_integer(key);
        },
    )
}

/// `GameRules.bootstrap(Registry<GameRule<?>>)` — the registry bootstrap
/// element, `ADVANCE_TIME`.
pub fn bootstrap() -> Arc<GameRuleErased> {
    built_in_registry()
        .by_id_arc(0)
        .expect("advance_time is the GAME_RULE bootstrap element")
        .clone()
}

/// `GameRules.codec(FeatureFlagSet enabledFeatures)` — `GameRuleMap.CODEC.xmap(
/// map -> new GameRules(enabledFeatures, map), gameRules -> gameRules.rules)`.
/// The GameRules wrapper (which reconciles the map against the enabled-feature
/// set) is deferred with the ServerLevel seam (`RivetTodo` below); the value
/// codec surface is the GameRuleMap CODEC — the accessor below.
///
/// RivetTodo(#388): `ServerLevel`/`MinecraftServer` unported — the `GameRules`
/// aggregate (its `rules` field, the enabled-feature-reconciling constructor,
/// `get`/`set`/`copy`/`setAll`/`setFromOther` with the `@Nullable ServerLevel`
/// `onGameRuleChanged` callback seam, `availableRules`, `getAsString`,
/// `visitGameRuleTypes`) lands with the server runtime. This unit ports the
/// game-rule values, the map and the registry.
///
/// `GameRuleMap.CODEC` — the `Codec<GameRuleMap>` the deferred GameRules
/// aggregate's `GameRuleMap.CODEC.xmap(...)` builds on (the dispatched-map
/// codec against the built-in GAME_RULE registry, wrapped by the
/// `ofTrusted`/`map` conversion in `game_rule_map::codec`).
pub fn game_rule_map_codec<Ops: rivet_serialization::dynamic_ops::DynamicOps + 'static>()
-> Arc<dyn rivet_serialization::codec::Codec<GameRuleMap, Ops>> {
    game_rule_map::codec::<Ops>(built_in_registry())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    /// The GAME_RULE registry holds exactly 59 rules in Paper declaration order
    /// (element id == insertion index), `advance_time` first.
    #[test]
    fn built_in_registry_registration_order() {
        let registry = built_in_registry();
        assert_eq!(registry.size(), 59);
        let key_set = registry.key_set();
        assert_eq!(key_set.len(), 59);
        assert_eq!(
            key_set[0],
            Identifier::with_default_namespace("advance_time")
        );
        assert_eq!(
            key_set[1],
            Identifier::with_default_namespace("advance_weather")
        );
        assert_eq!(
            key_set[13],
            Identifier::with_default_namespace("fire_spread_radius_around_player")
        );
        assert_eq!(
            key_set[58],
            Identifier::with_default_namespace("water_source_conversion")
        );
        // The bootstrap element is `advance_time` (id 0).
        assert_eq!(bootstrap().id(), "advance_time");
    }

    /// `gameRuleIndex` is assigned by `LAST_GAMERULE_INDEX++` at construction
    /// time, so the built-in rules' indices preserve declaration order. The
    /// counter is a process-global static (Java's `LAST_GAMERULE_INDEX`), and
    /// `cargo test` runs the `gamerules` test binaries on parallel threads —
    /// other tests' `bool_rule`/`int_rule` helpers can construct rules while
    /// the built-in `LazyLock` runs, so the built-ins are not a contiguous
    /// block of indices. The stable invariant under that interleaving is that
    /// the indices are strictly increasing across declaration order.
    #[test]
    fn game_rule_indices_are_insertion_order() {
        let registry = built_in_registry();
        let entries = registry.entry_set_ref();
        for pair in entries.windows(2) {
            assert!(pair[0].1.game_rule_index < pair[1].1.game_rule_index);
        }
    }

    /// Defaults match the Java `GameRules` declarations.
    #[test]
    fn built_in_defaults() {
        let registry = built_in_registry();
        let advance_time = registry
            .get_value_by_id(&Identifier::with_default_namespace("advance_time"))
            .unwrap();
        assert_eq!(advance_time.default_value, GameRuleValue::Bool(true));
        let fire_spread = registry
            .get_value_by_id(&Identifier::with_default_namespace(
                "fire_spread_radius_around_player",
            ))
            .unwrap();
        assert_eq!(fire_spread.default_value, GameRuleValue::Int(128));
        let immediate_respawn = registry
            .get_value_by_id(&Identifier::with_default_namespace("immediate_respawn"))
            .unwrap();
        assert_eq!(immediate_respawn.default_value, GameRuleValue::Bool(false));
        let max_minecart_speed = registry
            .get_value_by_id(&Identifier::with_default_namespace("max_minecart_speed"))
            .unwrap();
        assert_eq!(max_minecart_speed.default_value, GameRuleValue::Int(8));
        // `max_minecart_speed` requires the `minecart_improvements` flag.
        assert!(
            max_minecart_speed
                .required_features
                .contains(&MINECART_IMPROVEMENTS)
        );
    }

    /// `GameRules.visitGameRuleTypes` dispatches each rule to its typed visitor
    /// method (mirroring the deferred GameRules aggregate).
    #[test]
    fn built_in_visitors_dispatch_typed_methods() {
        struct CountingVisitor {
            booleans: usize,
            integers: usize,
        }
        impl GameRuleTypeVisitor for CountingVisitor {
            fn visit_boolean(&mut self, _game_rule: &GameRuleErased) {
                self.booleans += 1;
            }
            fn visit_integer(&mut self, _game_rule: &GameRuleErased) {
                self.integers += 1;
            }
        }
        let registry = built_in_registry();
        let mut visitor = CountingVisitor {
            booleans: 0,
            integers: 0,
        };
        for rule in registry.entry_set_ref() {
            rule.1.call_visitor(&mut visitor);
        }
        // 47 boolean rules + 12 integer rules (fire_spread_radius_around_player,
        // max_block_modifications, max_command_forks, max_command_sequence_length,
        // max_entity_cramming, max_minecart_speed, max_snow_accumulation_height,
        // players_nether_portal_creative_delay, players_nether_portal_default_delay,
        // players_sleeping_percentage, random_tick_speed, respawn_radius) — the
        // exact split of the 59 Java `registerBoolean`/`registerInteger` calls.
        assert_eq!(visitor.booleans, 47);
        assert_eq!(visitor.integers, 12);
    }

    /// The dispatched-map CODEC round-trips a game-rule map through JsonOps
    /// with per-key value codecs (`Codec.BOOL` / `Codec.intRange`).
    #[test]
    fn game_rule_map_codec_round_trips_through_json() {
        let ops = JsonOps::INSTANCE;
        let codec = game_rule_map_codec::<JsonOps>();

        let registry = built_in_registry();
        let advance_time = registry.by_id_arc(0).cloned().unwrap();
        // `GameRuleErased` is not `Clone` (it is identity-keyed by the Arc), so
        // resolve the rule by name to its id, then clone the registry-held Arc.
        let random_tick_speed_value = registry
            .get_value_by_id(&Identifier::with_default_namespace("random_tick_speed"))
            .unwrap();
        let random_tick_speed = registry
            .by_id_arc(registry.get_id(random_tick_speed_value))
            .cloned()
            .unwrap();

        let mut map = GameRuleMap::of();
        map.set(&advance_time, GameRuleValue::Bool(false));
        map.set(&random_tick_speed, GameRuleValue::Int(7));

        let encoded = codec
            .encode_start(&ops, &map)
            .result()
            .expect("encode should succeed")
            .clone();
        // Keys serialize by name, values by their typed codec. The key codec
        // is `byNameCodec`, whose RegistryOps encoding carries the full
        // namespaced `ResourceLocation` (Java's `byNameCodec` uses
        // `ResourceLocation.CODEC`, so keys round-trip as `minecraft:xxx`).
        //
        // Java's `DispatchedMapCodec.encode` emits entries in the input map's
        // (unspecified) iteration order, so the port does not canonicalize the
        // key order — assert the encoded object order-insensitively.
        let encoded_obj = encoded
            .as_object()
            .expect("encoded gamerules map is an object");
        assert_eq!(encoded_obj.len(), 2);
        assert_eq!(
            encoded_obj.get("minecraft:advance_time"),
            Some(&json!(false))
        );
        assert_eq!(
            encoded_obj.get("minecraft:random_tick_speed"),
            Some(&json!(7))
        );

        let parsed = codec.parse(&ops, &encoded);
        let decoded = parsed.result().expect("decode should succeed");
        assert_eq!(decoded.size(), 2);
        assert_eq!(decoded.get(&advance_time), Some(GameRuleValue::Bool(false)));
    }

    /// The dispatched-map codec rejects an out-of-range value for an integer
    /// rule (the key-dependent `Codec.intRange` applies per key).
    #[test]
    fn game_rule_map_codec_rejects_out_of_range_value() {
        let ops = JsonOps::INSTANCE;
        let codec = game_rule_map_codec::<JsonOps>();
        // `random_tick_speed` is `Codec.intRange(0, MAX)`; 11 is out of range.
        let result = codec.parse(&ops, &json!({ "random_tick_speed": -1 }));
        assert!(result.result().is_none());
    }
}
