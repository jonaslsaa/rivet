//! **Full** port of `net.minecraft.util.StringRepresentable` (113-line Java
//! interface, ported wholesale — the registry-core slice needs `Direction` and
//! `FrontAndTop` to implement it and to build `EnumCodec`s).
//!
//! PROVENANCE: leaf of the `mc.util` manifest unit (net.minecraft.util ->
//! rivet-util). RECONCILIATION: stays in this module when the full unit lands;
//! only the nested `StringRepresentableCodec`'s dependence on the DFU codec
//! surface (via `rivet-serialization::extra_codecs`) is a crate split.
//!
//! Java/Rust mapping notes:
//! - `getSerializedName()` -> `get_serialized_name()`.
//! - Java `Supplier<E[]>` is an eager call in every factory; the port takes
//!   `&'static [E]` (the enum's constants in declaration order).
//! - `Enum<E>.ordinal()` has no Rust intrinsic; the registry enums implement
//!   the `EnumOrdinal` helper trait below.
//! - The Java `Codec<E>` is ops-generic; the port pins `Ops: DynamicOps`
//!   exactly like every other `rivet-serialization` codec.

use rivet_serialization::codec::{self, Codec};
use rivet_serialization::data_result::DataResult;
use rivet_serialization::decoder::Decoder;
use rivet_serialization::dynamic_ops::{DynamicOps, Keyable};
use rivet_serialization::encoder::Encoder;
use rivet_serialization::extra_codecs;
use std::collections::HashMap;
use std::sync::Arc;

/// `StringRepresentable.PRE_BUILT_MAP_THRESHOLD` — values above this build a
/// name->value `HashMap`; at or below it `create_name_lookup` linear-scans.
pub const PRE_BUILT_MAP_THRESHOLD: usize = 16;

/// A `String -> value` lookup — `create_name_lookup`'s return type (Java's
/// `Function<String, @Nullable T>`).
type NameLookup<'a, T> = Arc<dyn Fn(&str) -> Option<&'a T> + Send + Sync + 'a>;

/// `StringRepresentable.getSerializedName()`.
pub trait StringRepresentable {
    fn get_serialized_name(&self) -> &str;
}

/// `Enum<E>.ordinal()` — the position of a variant in its `enum` declaration.
///
/// Java's `Enum` base class exposes this implicitly; Rust has no intrinsic, so
/// the registry enums implement it (0-based, in declaration order).
pub trait EnumOrdinal {
    fn ordinal(&self) -> usize;
}

// ---------------------------------------------------------------------------
// createNameLookup
// ---------------------------------------------------------------------------

/// `StringRepresentable.createNameLookup(T[])` — via the serialized name.
pub fn create_name_lookup<'a, T>(values: &'a [T]) -> NameLookup<'a, T>
where
    T: StringRepresentable + Send + Sync + 'a,
{
    create_name_lookup_with_converter(values, |v| v.get_serialized_name().to_string())
}

/// `StringRepresentable.createNameLookup(T[], Function<T, String> converter)`:
/// `valueArray.length > 16` builds a `HashMap<name, value>`; `<= 16` linear
/// scans. A duplicate name panics exactly where Java's `Collectors.toMap`
/// throws `IllegalStateException("Duplicate key ...")`.
pub fn create_name_lookup_with_converter<'a, T>(
    values: &'a [T],
    converter: impl Fn(&T) -> String + Send + Sync + 'a,
) -> NameLookup<'a, T>
where
    T: Send + Sync + 'a,
{
    if values.len() > PRE_BUILT_MAP_THRESHOLD {
        let mut map: HashMap<String, &'a T> = HashMap::new();
        for value in values {
            let name = converter(value);
            if map.insert(name.clone(), value).is_some() {
                panic!("Duplicate key {name}");
            }
        }
        Arc::new(move |id| map.get(id).copied())
    } else {
        Arc::new(move |id| values.iter().find(|v| converter(v) == *id))
    }
}

// ---------------------------------------------------------------------------
// StringRepresentableCodec
// ---------------------------------------------------------------------------

/// `StringRepresentable.StringRepresentableCodec<S>` — `orCompressed(
/// stringResolver(getSerializedName, nameResolver), idResolverCodec(idResolver,
/// fromInt, -1))`. `S` needs `Display` for Java's `String.valueOf` in the
/// `"Element with unknown id: "` message.
pub struct StringRepresentableCodec<S, Ops: DynamicOps + 'static> {
    inner: Arc<dyn Codec<S, Ops>>,
}

impl<S, Ops: DynamicOps + 'static> std::fmt::Debug for StringRepresentableCodec<S, Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "StringRepresentableCodec")
    }
}

impl<S: StringRepresentable + std::fmt::Display + Clone + 'static, Ops: DynamicOps + 'static>
    StringRepresentableCodec<S, Ops>
{
    /// Java's `StringRepresentableCodec` constructor — the caller supplies
    /// `from_int` (`i -> i >= 0 && i < valueArray.length ? valueArray[i] :
    /// null`), which is where Java's `valueArray` is captured.
    pub fn build(
        name_lookup: NameLookup<'static, S>,
        id_resolver: Arc<dyn Fn(&S) -> i32 + Send + Sync>,
        from_int: Arc<dyn Fn(i32) -> Option<S> + Send + Sync>,
    ) -> Self {
        let string_part = codec::string_resolver::<S, Ops>(
            Arc::new(|s: &S| Some(s.get_serialized_name().to_string())),
            Arc::new(move |name: &String| name_lookup(name).cloned()),
        );
        let id_part = extra_codecs::id_resolver_codec::<S, Ops>(id_resolver, from_int, -1);
        StringRepresentableCodec {
            inner: extra_codecs::or_compressed::<S, Ops>(string_part, id_part),
        }
    }
}

impl<S, Ops: DynamicOps + 'static> Encoder<S, Ops> for StringRepresentableCodec<S, Ops> {
    fn encode(&self, input: &S, ops: &Ops, prefix: &Ops::Output) -> DataResult<Ops::Output> {
        self.inner.encode(input, ops, prefix)
    }
}

impl<S, Ops: DynamicOps + 'static> Decoder<S, Ops> for StringRepresentableCodec<S, Ops> {
    fn decode(&self, ops: &Ops, input: &Ops::Output) -> DataResult<(S, Ops::Output)> {
        self.inner.decode(ops, input)
    }
}

impl<S, Ops: DynamicOps + 'static> Codec<S, Ops> for StringRepresentableCodec<S, Ops> {}

// ---------------------------------------------------------------------------
// EnumCodec
// ---------------------------------------------------------------------------

/// `StringRepresentable.EnumCodec<E>` — a `StringRepresentableCodec` built with
/// `idResolver = Enum.ordinal()`, plus the `byName` resolver.
/// The `EnumCodec.byName` resolver — `Function<String, @Nullable E>`.
type NameResolver<E> = Arc<dyn Fn(&str) -> Option<E> + Send + Sync>;

pub struct EnumCodec<E, Ops: DynamicOps + 'static> {
    codec: StringRepresentableCodec<E, Ops>,
    resolver: NameResolver<E>,
}

impl<E, Ops: DynamicOps + 'static> std::fmt::Debug for EnumCodec<E, Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "EnumCodec")
    }
}

impl<
    E: StringRepresentable + EnumOrdinal + std::fmt::Display + Copy + Send + Sync + 'static,
    Ops: DynamicOps + 'static,
> EnumCodec<E, Ops>
{
    /// Java's `EnumCodec(valueArray, nameResolver)` constructor — `super(...,
    /// rec -> rec.ordinal()); this.resolver = nameResolver;`.
    pub fn new(values: &'static [E], name_lookup: NameLookup<'static, E>) -> Self {
        let resolver_name_lookup = name_lookup.clone();
        let resolver: NameResolver<E> = Arc::new(move |name| resolver_name_lookup(name).copied());
        let name_lookup_for_build = name_lookup;
        let id_resolver = Arc::new(|e: &E| e.ordinal() as i32);
        let values_for_from_int: &'static [E] = values;
        let from_int = Arc::new(move |i: i32| {
            if i >= 0 && (i as usize) < values_for_from_int.len() {
                Some(values_for_from_int[i as usize])
            } else {
                None
            }
        });
        let codec = StringRepresentableCodec::build(name_lookup_for_build, id_resolver, from_int);
        EnumCodec { codec, resolver }
    }

    /// `EnumCodec.byName(String)` — `@Nullable`.
    pub fn by_name(&self, name: &str) -> Option<E> {
        (self.resolver)(name)
    }

    /// `EnumCodec.byName(String, E _default)` — `Objects.requireNonNullElse`.
    pub fn by_name_or(&self, name: &str, default: E) -> E {
        match (self.resolver)(name) {
            Some(value) => value,
            None => default,
        }
    }

    /// `EnumCodec.byName(String, Supplier<? extends E>)` —
    /// `Objects.requireNonNullElseGet`: the supplier runs ONLY when the name
    /// is unknown (lazy default).
    pub fn by_name_or_else(&self, name: &str, default: impl FnOnce() -> E) -> E {
        match (self.resolver)(name) {
            Some(value) => value,
            None => default(),
        }
    }
}

impl<E, Ops: DynamicOps + 'static> Encoder<E, Ops> for EnumCodec<E, Ops> {
    fn encode(&self, input: &E, ops: &Ops, prefix: &Ops::Output) -> DataResult<Ops::Output> {
        self.codec.encode(input, ops, prefix)
    }
}

impl<E, Ops: DynamicOps + 'static> Decoder<E, Ops> for EnumCodec<E, Ops> {
    fn decode(&self, ops: &Ops, input: &Ops::Output) -> DataResult<(E, Ops::Output)> {
        self.codec.decode(ops, input)
    }
}

impl<E, Ops: DynamicOps + 'static> Codec<E, Ops> for EnumCodec<E, Ops> {}

// ---------------------------------------------------------------------------
// fromEnum / fromValues factories
// ---------------------------------------------------------------------------

/// `StringRepresentable.fromEnum(Supplier<E[]>)` — identity-mapped
/// `from_enum_with_mapping`.
pub fn from_enum<E, Ops>(values: &'static [E]) -> EnumCodec<E, Ops>
where
    E: StringRepresentable + EnumOrdinal + std::fmt::Display + Copy + Send + Sync + 'static,
    Ops: DynamicOps + 'static,
{
    from_enum_with_mapping(values, Arc::new(|s: &str| s.to_string()))
}

/// `StringRepresentable.fromEnumWithMapping(Supplier<E[]>, Function<String,
/// String> converter)` — the converter is applied to each serialized name
/// before it enters `create_name_lookup`.
pub fn from_enum_with_mapping<E, Ops>(
    values: &'static [E],
    converter: Arc<dyn Fn(&str) -> String + Send + Sync>,
) -> EnumCodec<E, Ops>
where
    E: StringRepresentable + EnumOrdinal + std::fmt::Display + Copy + Send + Sync + 'static,
    Ops: DynamicOps + 'static,
{
    let per_value = converter.clone();
    let name_lookup =
        create_name_lookup_with_converter(values, move |v: &E| per_value(v.get_serialized_name()));
    EnumCodec::new(values, name_lookup)
}

/// `StringRepresentable.fromValues(Supplier<T[]>)` — a plain
/// `StringRepresentableCodec` (no `byName`); the id resolver is
/// `Util.createIndexLookup` (position-based), not ordinal.
pub fn from_values<T, Ops>(values: &'static [T]) -> Arc<dyn Codec<T, Ops>>
where
    T: StringRepresentable
        + std::fmt::Display
        + Copy
        + PartialEq
        + Eq
        + std::hash::Hash
        + Send
        + Sync
        + 'static,
    Ops: DynamicOps + 'static,
{
    let name_lookup = create_name_lookup(values);
    let index_lookup = crate::util::create_index_lookup(values);
    let id_resolver =
        Arc::new(move |e: &T| index_lookup.index_of(e).map(|i| i as i32).unwrap_or(-1));
    let values_for_from_int: &'static [T] = values;
    let from_int = Arc::new(move |i: i32| {
        if i >= 0 && (i as usize) < values_for_from_int.len() {
            Some(values_for_from_int[i as usize])
        } else {
            None
        }
    });
    let codec = StringRepresentableCodec::build(name_lookup, id_resolver, from_int);
    Arc::new(codec)
}

// ---------------------------------------------------------------------------
// keys
// ---------------------------------------------------------------------------

/// `StringRepresentable.keys(StringRepresentable[])` — a `Keyable` whose keys
/// are the serialized names.
#[derive(Debug, Clone, Copy)]
pub struct StringRepresentableKeys<T: 'static>(pub &'static [T]);

impl<T, Ops: DynamicOps + 'static> Keyable<Ops> for StringRepresentableKeys<T>
where
    T: StringRepresentable + Send + Sync + 'static,
{
    fn keys(&self, ops: &Ops) -> Vec<Ops::Output> {
        self.0
            .iter()
            .map(|v| ops.create_string(v.get_serialized_name().to_string()))
            .collect()
    }
}

/// `StringRepresentable.keys(StringRepresentable[])`.
pub fn keys<T: StringRepresentable + 'static>(values: &'static [T]) -> StringRepresentableKeys<T> {
    StringRepresentableKeys(values)
}
