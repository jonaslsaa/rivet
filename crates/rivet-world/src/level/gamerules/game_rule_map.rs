//! `net.minecraft.world.level.gamerules.GameRuleMap` — the game-rule value map
//! (Paper's array-backed access) and the `dispatchedMap` codec that serializes
//! it.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/level/
//! gamerules/GameRuleMap.java`.
//!
//! `GameRuleMap extends SavedData`: the port embeds the SavedData dirty flag
//! directly. The `SavedDataType<TYPE>` registration (`Identifier
//! "game_rules"`, `DataFixTypes.SAVED_DATA_GAME_RULES`) is deferred with the
//! `mc.world.level.saveddata` unit (`RivetTodo(#388)` marker below).
//!
//! ## The dispatched-map codec
//!
//! Java's `CODEC` is `Codec.<GameRule<?>, Object>dispatchedMap(
//! BuiltInRegistries.GAME_RULE.byNameCodec(), GameRule::valueCodec)` — a
//! `DispatchedMapCodec<K, V>` whose value codec is selected per key from the
//! key's own `valueCodec()`. DFU has no `dispatched_map` in this port, so the
//! codec below mirrors `BaseMapCodec.decode_map`/`encode_map` exactly (the same
//! duplicate/error semantics and `"{} missed input: {:?}"` error message as
//! `rivet-serialization::codecs::simple_map_codec`) with a key-dependent value
//! codec. The encode half is the map-builder form of `UnboundedMapCodec`.

use crate::level::gamerules::game_rule::{self, GameRuleErased, GameRuleValue};
use crate::level::gamerules::game_rules;
use rivet_registry::Registry;
use rivet_serialization::codec::Codec;
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::lifecycle::Lifecycle;
use rivet_serialization::pair::Pair;
use rivet_serialization::unit::Unit;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt::{self, Debug};
use std::rc::Rc;
use std::sync::Arc;

/// `GameRuleMap` — the `Reference2ObjectMap<GameRule<?>, Object>` value map
/// plus Paper's array-backed `idAccess`. The map is keyed by game-rule
/// identity (the registry-held `Arc`), so a `HashMap<Arc<GameRuleErased>, _>`
/// preserves Java's reference-key semantics.
///
/// The `map` is held as `Rc<RefCell<HashMap>>`: Java's `Builder.build()` and
/// the built `GameRuleMap` **share** the same `Reference2ObjectMap` object
/// (the private constructor assigns `this.map = map` directly), and
/// `copyOf`/`ofTrusted`/`of` wrap fresh maps. The port reproduces that shared
/// identity with an `Rc` clone of one `RefCell<HashMap>` — game state is
/// tick-thread-confined (OWNERSHIP.md D5), so the interior mutability is
/// single-thread `RefCell`, not `Arc<RwLock>`.
///
/// `GameRuleMap.TYPE` (deferred) — the `SavedDataType<GameRuleMap>` with key
/// `"game_rules"`. `SavedDataType` is owned by the `mc.world.level.saveddata`
/// unit; this unit defers the registration. The `CODEC` is still ported here
/// (the value codec is this unit's own surface).
///
/// RivetTodo(#388): `SavedDataType` unported — the `GameRuleMap.TYPE` stateless
/// registration (key `minecraft:game_rules`, `GameRuleMap::of` default, `CODEC`,
/// `DataFixTypes.SAVED_DATA_GAME_RULES`) lands with the saveddata unit.
///
/// No `Clone`: Java's `GameRuleMap` has no public copy — `copyOf` is the only
/// copy path and always produces a clean map (the private constructor never
/// calls `setDirty`). A `#[derive(Clone)]` would copy the embedded SavedData
/// `dirty` flag, diverging from the Java surface.
#[derive(Debug)]
pub struct GameRuleMap {
    /// `map` — `Reference2ObjectMap<GameRule<?>, Object>` (identity-keyed),
    /// shared by reference with the `Builder` that produced it (`Rc` clone of
    /// one `RefCell<HashMap>`).
    map: Rc<RefCell<HashMap<Arc<GameRuleErased>, GameRuleValue>>>,
    /// Paper array-backed access — one slot per constructed game rule, `None`
    /// when the rule has no value (the map never stores `null`).
    id_access: Vec<Option<GameRuleValue>>,
    /// `SavedData.dirty` — `setDirty()` on every mutation (Java sets the flag
    /// on `set`/`remove`; `reset`/`setFromIf`/`setGameRule` route through
    /// `set`).
    dirty: bool,
}

impl GameRuleMap {
    /// The private constructor — wraps a fresh `map` in the shared `Rc` and
    /// builds `idAccess` from its entries (Paper).
    fn from_map(map: HashMap<Arc<GameRuleErased>, GameRuleValue>) -> GameRuleMap {
        // Java's 59 built-in rules are static initializers of `GameRules`, so
        // `GameRule.LAST_GAMERULE_INDEX` is already 59 before any `GameRuleMap`
        // constructor can run — `idAccess` is guaranteed to cover every rule.
        // The Rust `BUILT_IN_RULES` LazyLock runs only on access, so force the
        // GAME_RULE bootstrap before sizing `id_access` to preserve that
        // invariant (a map built first would size `id_access` to 0 and every
        // subsequent `set`/`get`/`has`/`remove` would index out of bounds).
        game_rules::built_in_registry();
        let id_access = Self::id_access_from(&map);
        GameRuleMap {
            map: Rc::new(RefCell::new(map)),
            id_access,
            dirty: false,
        }
    }

    /// `new GameRuleMap(map)` with a **shared** map — `Builder.build()`'s path
    /// (Java's private constructor assigns `this.map = map` directly, so the
    /// builder and the built map share one map object). The `Rc` clone is the
    /// shared reference; `idAccess` is snapshotted at construction exactly as
    /// Java's array is, so a post-`build()` builder write is visible through
    /// the built map's map view (`keySet`/`size`) but not its array view.
    fn from_shared_map(
        map: Rc<RefCell<HashMap<Arc<GameRuleErased>, GameRuleValue>>>,
    ) -> GameRuleMap {
        game_rules::built_in_registry();
        let id_access = Self::id_access_from(&map.borrow());
        GameRuleMap {
            map,
            id_access,
            dirty: false,
        }
    }

    /// Paper's `idAccess` fill — one slot per constructed rule, `None` where the
    /// map has no value.
    fn id_access_from(
        map: &HashMap<Arc<GameRuleErased>, GameRuleValue>,
    ) -> Vec<Option<GameRuleValue>> {
        let mut id_access: Vec<Option<GameRuleValue>> =
            vec![None; game_rule::last_game_rule_index() as usize];
        for (rule, value) in map {
            id_access[rule.game_rule_index as usize] = Some(*value);
        }
        id_access
    }

    /// `GameRuleMap.ofTrusted(Map)` — wraps a decoded/encoded map.
    pub fn of_trusted(map: HashMap<Arc<GameRuleErased>, GameRuleValue>) -> GameRuleMap {
        Self::from_map(map)
    }

    /// `GameRuleMap.of()` — the empty map.
    pub fn of() -> GameRuleMap {
        Self::from_map(HashMap::new())
    }

    /// `GameRuleMap.of(Stream<GameRule<?>>)` — puts each rule's default.
    pub fn of_stream<'a>(rules: impl Iterator<Item = &'a Arc<GameRuleErased>>) -> GameRuleMap {
        let mut map = HashMap::new();
        for rule in rules {
            map.insert(rule.clone(), rule.default_value);
        }
        Self::from_map(map)
    }

    /// `GameRuleMap.copyOf(GameRuleMap)` — a shallow copy.
    ///
    /// Java's `copyOf` goes through the private constructor (`new GameRuleMap(
    /// new Reference2ObjectOpenHashMap<>(gameRuleMap.map))`), which never calls
    /// `setDirty` — a copy of a dirty source map is clean. The fresh
    /// `Reference2ObjectOpenHashMap` makes the copy **independent** of the
    /// source's shared map (no `Rc` sharing), so a later builder or built-map
    /// write does not leak into the copy. `from_map` rebuilds `id_access` and
    /// starts `dirty: false`. This is the ONLY copy path (the type has no
    /// `Clone`, mirroring Java's lack of a public copy), so the saved-data save
    /// cadence never sees a copied dirty flag.
    pub fn copy_of(game_rule_map: &GameRuleMap) -> GameRuleMap {
        Self::from_map(game_rule_map.map.borrow().clone())
    }

    /// `has(GameRule<?>)` — `idAccess[gameRuleIndex] != null`.
    pub fn has(&self, game_rule: &GameRuleErased) -> bool {
        self.id_access[game_rule.game_rule_index as usize].is_some()
    }

    /// `get(GameRule<T>)` — `@Nullable`; `idAccess[gameRuleIndex]`.
    pub fn get(&self, game_rule: &GameRuleErased) -> Option<GameRuleValue> {
        self.id_access[game_rule.game_rule_index as usize]
    }

    /// `set(GameRule<T>, T value)` — `setDirty(); map.put; idAccess[index] =
    /// value`.
    pub fn set(&mut self, game_rule: &Arc<GameRuleErased>, value: GameRuleValue) {
        self.dirty = true;
        self.map.borrow_mut().insert(game_rule.clone(), value);
        self.id_access[game_rule.game_rule_index as usize] = Some(value);
    }

    /// `reset(GameRule<T>)` — `set(gameRule, gameRule.defaultValue())`.
    pub fn reset(&mut self, game_rule: &Arc<GameRuleErased>) {
        let default_value = game_rule.default_value;
        self.set(game_rule, default_value);
    }

    /// `remove(GameRule<T>)` — `setDirty(); idAccess[index] = null; return
    /// map.remove`. Returns the removed value (`@Nullable`).
    pub fn remove(&mut self, game_rule: &GameRuleErased) -> Option<GameRuleValue> {
        self.dirty = true;
        self.id_access[game_rule.game_rule_index as usize] = None;
        self.map.borrow_mut().remove(game_rule)
    }

    /// `keySet()` — the identity-keyed map's key set.
    pub fn key_set(&self) -> Vec<Arc<GameRuleErased>> {
        self.map.borrow().keys().cloned().collect()
    }

    /// `size()`.
    pub fn size(&self) -> usize {
        self.map.borrow().len()
    }

    /// `withOther(GameRuleMap other)` — `copyOf(this)` then set every rule from
    /// `other` (`setFromIf(other, r -> true)`).
    pub fn with_other(&self, other: &GameRuleMap) -> GameRuleMap {
        let mut result = Self::copy_of(self);
        result.set_from_if(other, |_| true);
        result
    }

    /// `setFromIf(GameRuleMap other, Predicate<GameRule<?>>)` — for each key in
    /// `other`, if the predicate holds, `setGameRule(other, gameRule, this)`.
    pub fn set_from_if(
        &mut self,
        other: &GameRuleMap,
        predicate: impl Fn(&GameRuleErased) -> bool,
    ) {
        for game_rule in other.key_set() {
            if predicate(game_rule.as_ref()) {
                set_game_rule(other, &game_rule, self);
            }
        }
    }

    /// `SavedData.setDirty()` — the embedded dirty flag accessor (the SavedData
    /// `dirty` field; `isDirty`/`setDirty` are part of the deferred saveddata
    /// unit).
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// `GameRuleMap.Builder` — the `map` pre-population builder.
    pub fn builder() -> Builder {
        Builder::new()
    }
}

/// Java `toString()` — `this.map.toString()`. The underlying
/// `Reference2ObjectOpenHashMap` inherits `AbstractReference2ObjectMap
/// .toString()` (fastutil-8.5.18), which renders `{key=>value, ...}` — the
/// separator is `=>`, not `=` (the `(this map)` self-reference cases are
/// unreachable: keys are game rules and values are `Boolean`/`Integer`, never
/// the map itself). Each key renders its own `toString()` (the rule id) and the
/// value its `toString()` (`true`/`5`). Java's fastutil map iterates in
/// identity-hash slot order (non-deterministic across runs), so the Rust
/// `HashMap`'s iteration order is likewise unspecified. `k.id()` resolves the
/// built-in registry and panics for an unregistered rule, exactly Java's
/// `getIdentifier` NPE on the same path.
impl fmt::Display for GameRuleMap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let map = self.map.borrow();
        f.write_str("{")?;
        let mut first = true;
        for (k, v) in map.iter() {
            if !first {
                f.write_str(", ")?;
            }
            first = false;
            write!(f, "{}=>{}", k.id(), v)?;
        }
        f.write_str("}")
    }
}

/// Java `equals(Object)` — `Objects.equals(this.map, that.map)` on the
/// underlying identity-keyed `Reference2ObjectMap`: the same game-rule keys
/// (reference identity) mapped to equal values. The SavedData `dirty` flag and
/// the Paper `idAccess` array do not participate, so maps equal regardless of
/// their save state. `Rc` pointers are not compared — the borrowed `HashMap`
/// contents are (two maps built from independent `Rc`s with the same entries
/// are equal, exactly as two Java maps with equal maps are).
impl PartialEq for GameRuleMap {
    fn eq(&self, other: &Self) -> bool {
        *self.map.borrow() == *other.map.borrow()
    }
}

impl Eq for GameRuleMap {}

/// Java `hashCode()` — `Objects.hash(this.map)`. `HashMap` has no built-in
/// `Hash`, so the entries are hashed in a canonical order (by `game_rule_index`,
/// which does not require registry membership) consistent with the order-
/// insensitive `PartialEq`.
impl std::hash::Hash for GameRuleMap {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        let map = self.map.borrow();
        let mut entries: Vec<_> = map.iter().collect();
        entries.sort_by_key(|(k, _)| k.game_rule_index);
        for (k, v) in entries {
            k.hash(state);
            v.hash(state);
        }
    }
}

/// `GameRuleMap.setGameRule(GameRuleMap other, GameRule<T> gameRule,
/// GameRuleMap result)` — `result.set(gameRule, Objects.requireNonNull(
/// other.get(gameRule)))`. The `requireNonNull` (a game rule absent from
/// `other`) panics with a custom Rust message — Java's no-arg
/// `requireNonNull` throws an NPE with a `null` message, which the port does
/// not reproduce.
fn set_game_rule(other: &GameRuleMap, game_rule: &Arc<GameRuleErased>, result: &mut GameRuleMap) {
    let value = other.get(game_rule).unwrap_or_else(|| {
        panic!(
            "game rule '{}' must have a value in the source map",
            game_rule.id()
        )
    });
    result.set(game_rule, value);
}

/// `GameRuleMap.Builder` — `set(GameRule<T>, T value)` + `build()`.
pub struct Builder {
    map: Rc<RefCell<HashMap<Arc<GameRuleErased>, GameRuleValue>>>,
}

impl Default for Builder {
    fn default() -> Self {
        Self::new()
    }
}

impl Builder {
    /// `new Builder()` — the empty map.
    pub fn new() -> Builder {
        Builder {
            map: Rc::new(RefCell::new(HashMap::new())),
        }
    }

    /// `Builder.set(GameRule<T>, T value)` — returns `this` for chaining
    /// (`builder().set(a, v).set(b, w).build()`), exactly Java's shared
    /// reference return. Takes `&self` (not `&mut self`) so the builder stays
    /// usable after a `build()`, mirroring Java.
    pub fn set(&self, game_rule: &Arc<GameRuleErased>, value: GameRuleValue) -> &Self {
        self.map.borrow_mut().insert(game_rule.clone(), value);
        self
    }

    /// `Builder.build()` — `new GameRuleMap(this.map)`.
    ///
    /// Java's private `GameRuleMap` constructor assigns `this.map = map`
    /// directly, so the built map and the builder **share** the same map
    /// object. The port reproduces that shared identity with
    /// `Rc<RefCell<HashMap>>`: `build(&self)` returns a `GameRuleMap` holding
    /// an `Rc` clone of the builder's map (no copy). A post-`build()`
    /// `builder.set(...)` is therefore visible through the built map's
    /// `keySet()`/`size()` — but NOT its array-backed `idAccess` (snapshotted
    /// at construction; Java's own internal inconsistency) — and a
    /// `map.set(...)` on the built map is visible through the builder. Game
    /// state is tick-thread-confined (OWNERSHIP.md D5), so the interior
    /// mutability is single-thread `RefCell`, not `Arc<RwLock>`.
    pub fn build(&self) -> GameRuleMap {
        GameRuleMap::from_shared_map(self.map.clone())
    }
}

/// The dispatched-map value codec — a key-dependent value codec, wrapped as a
/// full `Codec<HashMap<Arc<GameRuleErased>, GameRuleValue>, Ops>` (the
/// `UnboundedMapCodec` shape with a per-key element codec).
pub struct DispatchedMapCodec<Ops: DynamicOps + 'static> {
    /// `BuiltInRegistries.GAME_RULE.byNameCodec()` — the key codec.
    key_codec: Arc<dyn Codec<Arc<GameRuleErased>, Ops>>,
    /// `GameRule::valueCodec` — the per-key value codec selection.
    _ops: std::marker::PhantomData<Ops>,
}

impl<Ops: DynamicOps + 'static> DispatchedMapCodec<Ops> {
    /// `Codec.dispatchedMap(keyCodec, valueCodecFunction)`.
    pub fn new(key_codec: Arc<dyn Codec<Arc<GameRuleErased>, Ops>>) -> DispatchedMapCodec<Ops> {
        DispatchedMapCodec {
            key_codec,
            _ops: std::marker::PhantomData,
        }
    }

    /// The per-key value codec — `GameRule::valueCodec.apply(key)`.
    fn value_codec(&self, key: &Arc<GameRuleErased>) -> Arc<dyn Codec<GameRuleValue, Ops>> {
        key.value_codec::<Ops>()
    }

    /// `BaseMapCodec.decode` — the shared `DispatchedMapCodec` decode: parse
    /// each key with the key codec, each value with the key's value codec,
    /// recording duplicates and failures.
    fn decode_map(
        &self,
        ops: &Ops,
        map_like: &dyn rivet_serialization::dynamic_ops::MapLike<Ops::Output>,
    ) -> DataResult<HashMap<Arc<GameRuleErased>, GameRuleValue>> {
        let mut read: HashMap<Arc<GameRuleErased>, GameRuleValue> = HashMap::new();
        let mut failed: Vec<Pair<Ops::Output, Ops::Output>> = Vec::new();

        let mut result: DataResult<Unit> =
            DataResult::success_with_lifecycle(Unit, Lifecycle::stable());
        let key_codec = self.key_codec.clone();

        for pair in map_like.entries() {
            let key = key_codec.parse(ops, &pair.first);
            // The value codec is key-dependent, so the value parse runs only
            // when the key parsed — `key.flatMap(k -> valueCodec(k).parse(
            // second))` in Java — and a failed key yields a failed entry.
            // `flat_map` consumes `key`, so the value uses a clone and `key` is
            // preserved for the `apply2stable`.
            let value: DataResult<GameRuleValue> = key
                .clone()
                .flat_map(|k| self.value_codec(&k).parse(ops, &pair.second));

            // `key.apply2stable(Pair::of, value)`.
            let entry_result: DataResult<(Arc<GameRuleErased>, GameRuleValue)> =
                DataResult::apply2_stable(
                    key,
                    |k: &Arc<GameRuleErased>, v: &GameRuleValue| (k.clone(), *v),
                    value,
                );

            if let Some(entry) = entry_result.clone().result_or_partial_silent() {
                let k = entry.0.clone();
                match read.entry(k.clone()) {
                    std::collections::hash_map::Entry::Occupied(_) => {
                        failed.push(pair.clone());
                        result = result.apply2_stable(
                            |_u: &Unit, _p: &Unit| Unit,
                            DataResult::error(format!("Duplicate entry for key: '{}'", k.id())),
                        );
                    }
                    std::collections::hash_map::Entry::Vacant(slot) => {
                        slot.insert(entry.1);
                    }
                }
            }
            if entry_result.is_error() {
                failed.push(pair.clone());
            }
            let r = result.clone();
            result = r.apply2_stable(|_u: &Unit, _p: &Unit| Unit, entry_result.map(|_| Unit));
        }

        let elements = read.clone();
        let errors = ops.create_map(failed);
        result
            .map(|_| elements.clone())
            .set_partial(elements)
            .map_error(|e| format!("{} missed input: {:?}", e, errors))
    }

    /// `BaseMapCodec.encode` — the per-key value codec selection.
    fn encode_map(
        &self,
        input: &HashMap<Arc<GameRuleErased>, GameRuleValue>,
        ops: &Ops,
        prefix: &mut dyn rivet_serialization::dynamic_ops::RecordBuilder<Output = Ops::Output>,
    ) {
        let key_codec = self.key_codec.clone();
        // Java's `DispatchedMapCodec.encode` iterates `input.entrySet()` in the
        // input map's own iteration order. For a `Reference2ObjectOpenHashMap`
        // (the built map) that order is identity-hash slot order, which depends
        // on `System.identityHashCode` (JVM-address-derived, not reproducible
        // across runs); the Rust `HashMap` iteration order is likewise
        // unspecified (per-process `RandomState`). Neither is byte-stable, so
        // this unit does NOT canonicalize — determinism here would be a
        // PORTING.md "improvement" over Java. The order is unobservable in the
        // encoded carriers anyway: the gamerules compound is stored in an NBT
        // `CompoundTag` (itself a `HashMap` that does not preserve entry order)
        // and readers fetch by key. The `game_rule_index` of an unregistered
        // key is never needed — the key reaches the key codec's real encode
        // error path below.
        for (k, v) in input.iter() {
            prefix.add_result_result(
                key_codec.encode_start(ops, k),
                self.value_codec(k).encode_start(ops, v),
            );
        }
    }
}

impl<Ops: DynamicOps + 'static> Debug for DispatchedMapCodec<Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "DispatchedMapCodec[{:?}]", self.key_codec)
    }
}

impl<Ops: DynamicOps + 'static>
    rivet_serialization::Decoder<HashMap<Arc<GameRuleErased>, GameRuleValue>, Ops>
    for DispatchedMapCodec<Ops>
{
    fn decode(
        &self,
        ops: &Ops,
        input: &Ops::Output,
    ) -> DataResult<(HashMap<Arc<GameRuleErased>, GameRuleValue>, Ops::Output)> {
        // `ops.getMap(input).flatMap(map -> decode(ops, map)).map(r ->
        // Pair.of(r, input))` — `DispatchedMapCodec.decode` does NOT pin the
        // lifecycle (`UnboundedMapCodec.decode` is the one that forces
        // `Lifecycle.stable()`).
        ops.get_map(input)
            .flat_map(|map| self.decode_map(ops, map.as_ref()))
            .map_owned(|r| (r, input.clone()))
    }
}

impl<Ops: DynamicOps + 'static>
    rivet_serialization::Encoder<HashMap<Arc<GameRuleErased>, GameRuleValue>, Ops>
    for DispatchedMapCodec<Ops>
{
    fn encode(
        &self,
        input: &HashMap<Arc<GameRuleErased>, GameRuleValue>,
        ops: &Ops,
        prefix: &Ops::Output,
    ) -> DataResult<Ops::Output> {
        let mut builder = ops.map_builder();
        self.encode_map(input, ops, &mut *builder);
        builder.build(Some(prefix.clone()))
    }
}

impl<Ops: DynamicOps + 'static> Codec<HashMap<Arc<GameRuleErased>, GameRuleValue>, Ops>
    for DispatchedMapCodec<Ops>
{
}

/// `GameRuleMap.CODEC` — the dispatched-map codec `.xmap(ofTrusted, map)`,
/// ops-generic. The decode map's key type is the identity-keyed
/// `HashMap` the map-builder round-trips.
pub fn codec<Ops: DynamicOps + 'static>(
    game_rule_registry: &Registry<GameRuleErased>,
) -> Arc<dyn Codec<HashMap<Arc<GameRuleErased>, GameRuleValue>, Ops>> {
    Arc::new(DispatchedMapCodec::new(
        game_rule_registry.by_name_codec::<Ops>(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::gamerules::game_rules::built_in_registry;

    /// The Java `toString`/`equals`/`hashCode` surface: `toString()` renders
    /// `{id=>value, ...}` (the fastutil `AbstractReference2ObjectMap`
    /// separator is `=>`); two maps with the same rule entries are equal and
    /// hash-equal regardless of their SavedData dirty state (Java's
    /// `Objects.equals(this.map, that.map)` / `Objects.hash(this.map)` ignore
    /// `dirty` and `idAccess`).
    #[test]
    fn to_string_equals_hash_ignore_dirty_and_match_map() {
        let registry = built_in_registry();
        let advance_time = registry.by_id_arc(0).cloned().unwrap();

        let mut map = GameRuleMap::of();
        map.set(&advance_time, GameRuleValue::Bool(true));
        // `set` marks the map dirty; a copy-of is clean but equal in value.
        assert!(map.is_dirty());
        let copy = GameRuleMap::copy_of(&map);
        assert!(!copy.is_dirty());
        assert_eq!(map, copy);
        // `Objects.hash(this.map)` — equal maps hash equal (dirty ignored).
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut a = DefaultHasher::new();
        let mut b = DefaultHasher::new();
        map.hash(&mut a);
        copy.hash(&mut b);
        assert_eq!(a.finish(), b.finish());

        // Java `toString()` is `this.map.toString()` — fastutil's
        // `AbstractReference2ObjectMap.toString()` emits `{id=>value}` (the
        // separator is `=>`, not `=`).
        assert_eq!(map.to_string(), "{advance_time=>true}");

        // A second entry pins the fastutil `", "` joining. Iteration order is
        // unspecified (`HashMap` random state, mirroring Java's identity-hash
        // slot order), so assert the pieces rather than the full string.
        let advance_weather = registry.by_id_arc(1).cloned().unwrap();
        let mut two = GameRuleMap::of();
        two.set(&advance_time, GameRuleValue::Bool(true));
        two.set(&advance_weather, GameRuleValue::Bool(false));
        let s = two.to_string();
        assert!(s.starts_with('{') && s.ends_with('}'));
        let parts: Vec<&str> = s[1..s.len() - 1].split(", ").collect();
        assert_eq!(parts.len(), 2);
        assert!(parts.iter().all(|p| p.contains("=>")));
        assert!(parts.contains(&"advance_time=>true"));
        assert!(parts.contains(&"advance_weather=>false"));

        // A map with a different value is not equal (Java `Objects.equals`).
        let mut other = GameRuleMap::of();
        other.set(&advance_time, GameRuleValue::Bool(false));
        assert_ne!(map, other);
    }

    /// `has`/`get`/`set`/`remove`/`reset` over the Paper array-backed idAccess.
    #[test]
    fn map_has_get_set_remove_reset() {
        let registry = built_in_registry();
        // The first built-in rule is `advance_time` (registration order).
        let advance_time = registry.by_id_arc(0).cloned().unwrap();
        let mut map = GameRuleMap::of();
        assert!(!map.has(&advance_time));
        assert_eq!(map.get(&advance_time), None);
        // Set a non-default value (`advance_time` defaults to `Bool(true)`) so
        // the reset below is observable.
        map.set(&advance_time, GameRuleValue::Bool(false));
        assert!(map.has(&advance_time));
        assert_eq!(map.get(&advance_time), Some(GameRuleValue::Bool(false)));
        // set marks the SavedData dirty flag.
        assert!(map.is_dirty());
        map.reset(&advance_time);
        // reset restores the rule's default and routes through set, which keeps
        // the map dirty.
        assert_eq!(map.get(&advance_time), Some(GameRuleValue::Bool(true)));
        assert!(map.is_dirty());
        let removed = map.remove(&advance_time);
        assert_eq!(removed, Some(GameRuleValue::Bool(true)));
        assert!(!map.has(&advance_time));
    }

    /// `Builder` — Java's `build()` shares the builder's live map object with
    /// the built `GameRuleMap` (the private constructor assigns `this.map =
    /// map` directly). The port reproduces that shared identity with
    /// `Rc<RefCell<HashMap>>` (tick-thread-confined value state, OWNERSHIP.md
    /// D5). A post-`build()` builder write is visible through the built map's
    /// `keySet()`/`size()` — but NOT its array-backed `idAccess` (snapshotted
    /// at construction; Java's own internal inconsistency) — and a built-map
    /// write is visible through the builder. `copyOf` still copies (fresh
    /// `Reference2ObjectOpenHashMap`), so a copy is independent of the shared
    /// map.
    #[test]
    fn builder_build_shares_map_identity_with_post_build_mutation() {
        let registry = built_in_registry();
        let advance_time = registry.by_id_arc(0).cloned().unwrap();
        let advance_weather = registry.by_id_arc(1).cloned().unwrap();

        let builder = GameRuleMap::builder();
        let mut map = builder
            .set(&advance_time, GameRuleValue::Bool(true))
            .build();
        // Every pre-build write is visible in both the map view and idAccess view.
        assert!(map.has(&advance_time));
        assert_eq!(map.get(&advance_time), Some(GameRuleValue::Bool(true)));
        assert_eq!(map.size(), 1);

        // Post-build builder.set is visible through the built map's map view
        // (shared identity) — but NOT its array-backed idAccess.
        builder.set(&advance_weather, GameRuleValue::Bool(false));
        assert_eq!(map.size(), 2);
        assert!(map.key_set().contains(&advance_weather));
        assert!(!map.has(&advance_weather));
        assert_eq!(map.get(&advance_weather), None);

        // A built-map set is visible through the builder (shared identity in
        // the other direction): a second build() recomputes idAccess from the
        // now-shared map, so the new map sees the rule in both views.
        map.set(&advance_weather, GameRuleValue::Bool(true));
        let rebuilt = builder.build();
        assert!(rebuilt.has(&advance_weather));
        assert_eq!(
            rebuilt.get(&advance_weather),
            Some(GameRuleValue::Bool(true))
        );
        assert_eq!(rebuilt.size(), 2);

        // copyOf copies the shared map into an independent map: a later builder
        // write is invisible to the copy.
        let copy = GameRuleMap::copy_of(&map);
        assert_eq!(copy, map);
        builder.set(&advance_time, GameRuleValue::Bool(false));
        assert_eq!(copy.get(&advance_time), Some(GameRuleValue::Bool(true)));
        assert_eq!(copy.size(), 2);
    }
}
