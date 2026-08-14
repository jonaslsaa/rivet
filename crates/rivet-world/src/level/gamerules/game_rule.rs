//! `net.minecraft.world.level.gamerules.GameRule` — the immutable game-rule key
//! and its value, argument, codec and visitor-caller surface.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/level/
//! gamerules/GameRule.java`. `GameRule<T>` is generic over the value type
//! (`Boolean` or `Integer`); the port erases the wildcard `GameRule<?>` to
//! [`GameRuleErased`] (the same erased-wildcard pattern as
//! `levelgen::feature::ConfiguredFeatureErased`). The value is [`GameRuleValue`]
//! and the value-typed seams (`argument`, `valueCodec`,
//! `commandResultFunction`, `visitorCaller`, `defaultValue`) are erased to the
//! union of the two value types.

use crate::flag::feature_flag_set::FeatureFlagSet;
use crate::level::gamerules::game_rule_category::GameRuleCategory;
use crate::level::gamerules::game_rule_type::GameRuleType;
use crate::level::gamerules::game_rule_type_visitor::GameRuleTypeVisitor;
use rivet_brigadier::ImmutableStringReader;
use rivet_brigadier::arguments::ArgumentType;
use rivet_brigadier::exceptions::CommandSyntaxException;
use rivet_brigadier::string_reader::StringReader;
use rivet_registry::Identifier;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::DynamicOps;
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicI32, Ordering};

/// `GameRule.LAST_GAMERULE_INDEX` (Paper) — the static counter assigning each
/// constructed `GameRule` its `gameRuleIndex`. Java's `static int` is mutated by
/// `gameRuleIndex = LAST_GAMERULE_INDEX++` at construction; all 59 built-in
/// rules are constructed inside the GAME_RULE registry bootstrap, so an atomic
/// fetch-add reproduces the sequential Paper counter exactly.
static LAST_GAMERULE_INDEX: AtomicI32 = AtomicI32::new(0);

/// `GameRule.LAST_GAMERULE_INDEX` — the next `gameRuleIndex` to assign.
pub fn last_game_rule_index() -> i32 {
    LAST_GAMERULE_INDEX.load(Ordering::Relaxed)
}

/// The erased `GameRule<?>` wildcard value — `Boolean` or `Integer`.
///
/// Java's `Object`-valued `Reference2ObjectMap` erases the value; the port
/// keeps the two value types explicit so the dispatched-map codec and the
/// value-typed accessors type-check. `Display` mirrors Java `toString()`
/// (`Boolean.toString()` / `Integer.toString()`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GameRuleValue {
    /// `GameRule<Boolean>`.
    Bool(bool),
    /// `GameRule<Integer>`.
    Int(i32),
}

impl fmt::Display for GameRuleValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GameRuleValue::Bool(b) => write!(f, "{b}"),
            GameRuleValue::Int(i) => write!(f, "{i}"),
        }
    }
}

/// The erased `ArgumentType<T>` — `BoolArgumentType` or `IntegerArgumentType`.
///
/// The brigadier argument types are generic over `T`, so the erased rule holds
/// the concrete `Arc<dyn ArgumentType<...>>` for its value type and dispatches
/// `parse` (the only value-typed `argument` use, in `deserialize`) to it.
pub enum ArgumentErased {
    /// `BoolArgumentType.bool()`.
    Bool(Arc<dyn ArgumentType<bool>>),
    /// `IntegerArgumentType.integer(min, max)`.
    Int(Arc<dyn ArgumentType<i32>>),
}

impl ArgumentErased {
    /// `this.argument.parse(reader)` — parse a rule value from the reader.
    pub fn parse(
        &self,
        reader: &mut StringReader,
    ) -> Result<GameRuleValue, CommandSyntaxException<'static>> {
        match self {
            ArgumentErased::Bool(argument) => argument.parse(reader).map(GameRuleValue::Bool),
            ArgumentErased::Int(argument) => argument.parse(reader).map(GameRuleValue::Int),
        }
    }
}

// `dyn ArgumentType<T>` does not implement `Debug`, so the erased debug name is
// the variant itself (the concrete argument type is fixed per rule value type).
impl fmt::Debug for ArgumentErased {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ArgumentErased::Bool(_) => f.write_str("Bool(BoolArgumentType)"),
            ArgumentErased::Int(_) => f.write_str("Int(IntegerArgumentType)"),
        }
    }
}

/// The erased `Codec<T>` value codec — the per-rule value codec factory.
///
/// Java stores the concrete `Codec<T>` (`Codec.BOOL` or
/// `Codec.intRange(min, max)`) on the rule. The Rust port pins the ops at use
/// time, so the codec is reconstructed from this metadata by [`Self::codec`] —
/// functionally identical for the stateless bool/int-range codecs (which is all
/// the gamerules unit uses).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameRuleValueCodec {
    /// `Codec.BOOL`.
    Bool,
    /// `Codec.intRange(min, max)`.
    Int { min: i32, max: i32 },
}

impl GameRuleValueCodec {
    /// `GameRule.valueCodec()` — the rule's value codec (ops-generic). The
    /// `xmap` encode half panics on a mismatched variant, the Rust analog of
    /// Java's `ClassCastException` when the wrong boxed type reaches a typed
    /// codec (impossible through the dispatched map, which selects the codec by
    /// the key's own type).
    pub fn codec<Ops: DynamicOps + 'static>(&self) -> Arc<dyn Codec<GameRuleValue, Ops>> {
        match self {
            GameRuleValueCodec::Bool => codec::xmap(
                codec::bool_codec::<Ops>(),
                Arc::new(|b: &bool| GameRuleValue::Bool(*b)),
                Arc::new(|value: &GameRuleValue| match value {
                    GameRuleValue::Bool(b) => *b,
                    GameRuleValue::Int(_) => {
                        panic!("a boolean rule codec cannot encode an int value")
                    }
                }),
            ),
            GameRuleValueCodec::Int { min, max } => codec::xmap(
                codec::int_range::<Ops>(*min, *max),
                Arc::new(|i: &i32| GameRuleValue::Int(*i)),
                Arc::new(|value: &GameRuleValue| match value {
                    GameRuleValue::Int(i) => *i,
                    GameRuleValue::Bool(_) => {
                        panic!("an integer rule codec cannot encode a bool value")
                    }
                }),
            ),
        }
    }
}

/// `GameRules.VisitorCaller<T>` — the typed visitor dispatch, erased.
///
/// Java's `VisitorCaller<T>` is `void call(GameRuleTypeVisitor visitor,
/// GameRule<T> key)`; the concrete closures are `GameRuleTypeVisitor::visitBoolean`
/// / `visitInteger` method references, so the erased form is a closure that
/// calls the typed visitor method with the rule.
pub type VisitorCaller = Arc<dyn Fn(&mut dyn GameRuleTypeVisitor, &GameRuleErased) + Send + Sync>;

/// `net.minecraft.world.level.gamerules.GameRule<?>` — the erased wildcard.
///
/// The registry stores `Arc<GameRuleErased>`; `game_rule_index` (Paper) is the
/// construction-order index into the `GameRuleMap` array-backed `idAccess`.
pub struct GameRuleErased {
    /// `category`.
    pub category: GameRuleCategory,
    /// `gameRuleType`.
    pub game_rule_type: GameRuleType,
    /// `argument`.
    pub argument: ArgumentErased,
    /// `visitorCaller`.
    pub visitor_caller: VisitorCaller,
    /// `valueCodec`.
    pub value_codec: GameRuleValueCodec,
    /// `commandResultFunction` (erased `ToIntFunction<T>`).
    pub command_result_function: Arc<dyn Fn(&GameRuleValue) -> i32 + Send + Sync>,
    /// `defaultValue`.
    pub default_value: GameRuleValue,
    /// `requiredFeatures`.
    pub required_features: FeatureFlagSet,
    /// `gameRuleIndex` (Paper) — the construction-order index for array-backed
    /// `GameRuleMap` access.
    pub game_rule_index: i32,
}

impl GameRuleErased {
    /// The `GameRule(...)` constructor — assigns `gameRuleIndex =
    /// LAST_GAMERULE_INDEX++` (Paper).
    #[allow(clippy::too_many_arguments)] // mirrors the 8-field Java constructor 1:1
    pub fn new(
        category: GameRuleCategory,
        game_rule_type: GameRuleType,
        argument: ArgumentErased,
        visitor_caller: VisitorCaller,
        value_codec: GameRuleValueCodec,
        command_result_function: Arc<dyn Fn(&GameRuleValue) -> i32 + Send + Sync>,
        default_value: GameRuleValue,
        required_features: FeatureFlagSet,
    ) -> GameRuleErased {
        GameRuleErased {
            category,
            game_rule_type,
            argument,
            visitor_caller,
            value_codec,
            command_result_function,
            default_value,
            required_features,
            game_rule_index: LAST_GAMERULE_INDEX.fetch_add(1, Ordering::Relaxed),
        }
    }

    /// `id()` — `this.getIdentifier().toShortString()`.
    pub fn id(&self) -> String {
        self.get_identifier().to_short_string()
    }

    /// `getIdentifier()` — `Objects.requireNonNull(BuiltInRegistries.GAME_RULE
    /// .getKey(this))`; a rule that is not in the GAME_RULE registry panics
    /// (Java's `NullPointerException`).
    pub fn get_identifier(&self) -> Identifier {
        crate::level::gamerules::game_rules::built_in_registry()
            .get_key(self)
            .expect("game rule must be registered in the GAME_RULE registry")
    }

    /// `getIdentifierWithFallback()` — `requireNonNullElse(..., withDefaultNamespace(
    /// "unregistered_sadface"))`.
    pub fn get_identifier_with_fallback(&self) -> Identifier {
        crate::level::gamerules::game_rules::built_in_registry()
            .get_key(self)
            .unwrap_or_else(|| Identifier::with_default_namespace("unregistered_sadface"))
    }

    /// `getDescriptionId()` — `Util.makeDescriptionId("gamerule",
    /// this.getIdentifier())`.
    pub fn get_description_id(&self) -> String {
        make_description_id("gamerule", &self.get_identifier())
    }

    /// `serialize(T value)` — `value.toString()`.
    pub fn serialize(&self, value: &GameRuleValue) -> String {
        value.to_string()
    }

    /// `deserialize(String value)` — parses through `this.argument`, erroring
    /// on trailing characters (`"Failed to deserialize; trailing characters"`)
    /// or a `CommandSyntaxException` (`"Failed to deserialize"`).
    pub fn deserialize(&self, value: &str) -> DataResult<GameRuleValue> {
        let mut reader = StringReader::new(value);
        let result = self.argument.parse(&mut reader);
        match result {
            Ok(result) => {
                if reader.can_read() {
                    // `DataResult.error(() -> "Failed to deserialize; trailing
                    // characters", result)` — the parsed value is the partial.
                    DataResult::error_with_partial(
                        "Failed to deserialize; trailing characters",
                        result,
                    )
                } else {
                    DataResult::success(result)
                }
            }
            Err(_) => DataResult::error("Failed to deserialize"),
        }
    }

    /// `valueClass()` — the value's class (`Boolean.class` or
    /// `Integer.class`), from `defaultValue.getClass()`. Erased to the value
    /// type (`GameRuleType` distinguishes the two `Class<T>` results).
    pub fn value_class(&self) -> GameRuleType {
        self.game_rule_type
    }

    /// `callVisitor(GameRuleTypeVisitor)` — `this.visitorCaller.call(visitor,
    /// this)`.
    pub fn call_visitor(&self, visitor: &mut dyn GameRuleTypeVisitor) {
        (self.visitor_caller)(visitor, self);
    }

    /// `getCommandResult(T value)` — `commandResultFunction.applyAsInt(value)`.
    pub fn get_command_result(&self, value: &GameRuleValue) -> i32 {
        (self.command_result_function)(value)
    }

    /// `category()`.
    pub fn category(&self) -> &GameRuleCategory {
        &self.category
    }

    /// `gameRuleType()`.
    pub fn game_rule_type(&self) -> GameRuleType {
        self.game_rule_type
    }

    /// `argument()` — the erased argument type.
    pub fn argument(&self) -> &ArgumentErased {
        &self.argument
    }

    /// `valueCodec()` — the rule's value codec (ops-generic).
    pub fn value_codec<Ops: DynamicOps + 'static>(&self) -> Arc<dyn Codec<GameRuleValue, Ops>> {
        self.value_codec.codec::<Ops>()
    }

    /// `defaultValue()`.
    pub fn default_value(&self) -> &GameRuleValue {
        &self.default_value
    }

    /// `requiredFeatures()`.
    pub fn required_features(&self) -> &FeatureFlagSet {
        &self.required_features
    }

    /// `FeatureElement.isEnabled(enabledFeatures)` —
    /// `requiredFeatures().isSubsetOf(enabledFeatures)` (the `FeatureElement`
    /// trait is deferred with `RivetTodo(#387)`; the subset test is inlined).
    pub fn is_enabled(&self, enabled_features: &FeatureFlagSet) -> bool {
        self.required_features.is_subset_of(enabled_features)
    }
}

/// Java equality for `GameRule<?>` is reference identity (the registry's
/// `HashMap<GameRule<?>, ...>` is identity-keyed). Every constructed rule gets a
/// unique `game_rule_index` (`LAST_GAMERULE_INDEX++`), so keying `PartialEq`/
/// `Eq`/`Hash` on that index reproduces identity semantics exactly — the same
/// erased-rule-key pattern as `levelgen::feature::ConfiguredFeatureErased`.
impl PartialEq for GameRuleErased {
    fn eq(&self, other: &Self) -> bool {
        self.game_rule_index == other.game_rule_index
    }
}

impl Eq for GameRuleErased {}

impl std::hash::Hash for GameRuleErased {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.game_rule_index.hash(state);
    }
}

/// `toString()` — `this.id()` (the registry key's short form).
impl fmt::Display for GameRuleErased {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.id())
    }
}

impl fmt::Debug for GameRuleErased {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GameRuleErased")
            .field("id", &self.id())
            .field("category", &self.category)
            .field("game_rule_type", &self.game_rule_type)
            .field("default_value", &self.default_value)
            .field("game_rule_index", &self.game_rule_index)
            .finish()
    }
}

/// `Util.makeDescriptionId(prefix, Identifier location)` — `prefix +
/// "." + namespace + "." + path.replace('/', '.')`. Java's null-location branch
/// (`prefix + ".unregistered_sadface"`) is unreachable here: the caller uses
/// the panicking `getIdentifier`, exactly Java's `getDescriptionId`.
fn make_description_id(prefix: &str, location: &Identifier) -> String {
    format!(
        "{}.{}.{}",
        prefix,
        location.namespace(),
        location.path().replace('/', ".")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::gamerules::game_rule_category;
    use crate::level::gamerules::game_rule_type::GameRuleType;
    use rivet_brigadier::arguments::bool_argument_type::BoolArgumentType;
    use rivet_brigadier::arguments::integer_argument_type::IntegerArgumentType;

    fn bool_rule(default_value: bool) -> GameRuleErased {
        GameRuleErased::new(
            (*game_rule_category::PLAYER).clone(),
            GameRuleType::Bool,
            ArgumentErased::Bool(BoolArgumentType::bool()),
            Arc::new(
                |visitor: &mut dyn GameRuleTypeVisitor, key: &GameRuleErased| {
                    visitor.visit_boolean(key)
                },
            ),
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
        )
    }

    fn int_rule(default_value: i32, min: i32, max: i32) -> GameRuleErased {
        GameRuleErased::new(
            (*game_rule_category::PLAYER).clone(),
            GameRuleType::Int,
            ArgumentErased::Int(IntegerArgumentType::integer_with_bounds(min, max)),
            Arc::new(
                |visitor: &mut dyn GameRuleTypeVisitor, key: &GameRuleErased| {
                    visitor.visit_integer(key)
                },
            ),
            GameRuleValueCodec::Int { min, max },
            Arc::new(|value: &GameRuleValue| match value {
                GameRuleValue::Int(i) => *i,
                GameRuleValue::Bool(_) => panic!("integer rule command result"),
            }),
            GameRuleValue::Int(default_value),
            FeatureFlagSet::of(),
        )
    }

    /// `GameRule.serialize` is `value.toString()` — `true`/`5`.
    #[test]
    fn serialize_is_value_to_string() {
        let rule = bool_rule(true);
        assert_eq!(rule.serialize(&GameRuleValue::Bool(true)), "true");
        assert_eq!(rule.serialize(&GameRuleValue::Bool(false)), "false");
        let int_rule = int_rule(0, 0, 10);
        assert_eq!(int_rule.serialize(&GameRuleValue::Int(5)), "5");
        assert_eq!(int_rule.serialize(&GameRuleValue::Int(-3)), "-3");
    }

    /// `GameRule.deserialize` parses through the argument type and rejects
    /// trailing characters.
    #[test]
    fn deserialize_accepts_exact_parse_and_rejects_trailing() {
        let rule = bool_rule(true);
        assert_eq!(
            rule.deserialize("true").result().copied(),
            Some(GameRuleValue::Bool(true))
        );
        // Trailing characters error (the parsed value is the partial). Brigadier
        // reads one unquoted string, so the trailing character needs a space
        // separator ("truex" is a single token and fails as an invalid bool).
        let trailing = rule.deserialize("true x");
        assert!(trailing.result().is_none());
        // `resultOrPartial` still yields the parsed `true`.
        assert_eq!(
            trailing.result_or_partial_silent(),
            Some(GameRuleValue::Bool(true))
        );
        // A `CommandSyntaxException` is `"Failed to deserialize"` with no value.
        assert_eq!(
            rule.deserialize("not-a-bool")
                .error_ref()
                .unwrap()
                .message(),
            "Failed to deserialize"
        );
    }

    /// Integer rules parse within their bounds.
    #[test]
    fn deserialize_integer_checks_bounds() {
        let rule = int_rule(0, 0, 10);
        assert_eq!(
            rule.deserialize("7").result().copied(),
            Some(GameRuleValue::Int(7))
        );
        // Out-of-bounds is a CommandSyntaxException -> "Failed to deserialize".
        assert_eq!(
            rule.deserialize("11").error_ref().unwrap().message(),
            "Failed to deserialize"
        );
    }

    /// `callVisitor` dispatches the typed visitor method by value type.
    #[test]
    fn call_visitor_dispatches_to_typed_method() {
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
        let bool_rule = bool_rule(true);
        let int_rule = int_rule(0, 0, 10);
        let mut visitor = CountingVisitor {
            booleans: 0,
            integers: 0,
        };
        bool_rule.call_visitor(&mut visitor);
        int_rule.call_visitor(&mut visitor);
        assert_eq!(visitor.booleans, 1);
        assert_eq!(visitor.integers, 1);
    }

    /// `getCommandResult` — `b -> b ? 1 : 0` / `i -> i`.
    #[test]
    fn command_results_match_java() {
        let bool_rule = bool_rule(true);
        assert_eq!(bool_rule.get_command_result(&GameRuleValue::Bool(true)), 1);
        assert_eq!(bool_rule.get_command_result(&GameRuleValue::Bool(false)), 0);
        let int_rule = int_rule(0, 0, 10);
        assert_eq!(int_rule.get_command_result(&GameRuleValue::Int(7)), 7);
        assert_eq!(int_rule.get_command_result(&GameRuleValue::Int(-2)), -2);
    }

    /// The value codec round-trips through JsonOps and validates the int range.
    #[test]
    fn value_codec_round_trips_through_json() {
        use rivet_serialization::json_ops::JsonOps;
        let ops = JsonOps::INSTANCE;

        let bool_codec = bool_rule(true).value_codec::<JsonOps>();
        assert_eq!(
            bool_codec
                .parse(&ops, &serde_json::json!(true))
                .result()
                .copied(),
            Some(GameRuleValue::Bool(true))
        );
        assert_eq!(
            bool_codec
                .encode_start(&ops, &GameRuleValue::Bool(false))
                .result(),
            Some(&serde_json::json!(false))
        );

        let int_codec = int_rule(0, 0, 10).value_codec::<JsonOps>();
        assert_eq!(
            int_codec
                .parse(&ops, &serde_json::json!(7))
                .result()
                .copied(),
            Some(GameRuleValue::Int(7))
        );
        // Out-of-range values fail the `Codec.intRange` validation on both
        // decode and encode.
        assert!(
            int_codec
                .parse(&ops, &serde_json::json!(11))
                .result()
                .is_none()
        );
        assert!(
            int_codec
                .encode_start(&ops, &GameRuleValue::Int(11))
                .result()
                .is_none()
        );
    }

    /// `makeDescriptionId` — `"gamerule.minecraft.<path>"` with `/` -> `.`.
    #[test]
    fn make_description_id_formats_prefix_namespace_path() {
        let id = Identifier::with_default_namespace("do_fire_tick");
        assert_eq!(
            make_description_id("gamerule", &id),
            "gamerule.minecraft.do_fire_tick"
        );
    }
}
