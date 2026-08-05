//! Port of Mojang DataFixerUpper (DFU): `com.mojang.serialization` plus the
//! `com.mojang.datafixers.util` helpers it is built on (`Pair`, `Either`,
//! `Unit`).
//!
//! Translation notes (see PORTING.md):
//! - Java `DynamicOps<T>` (output type parameter) maps to an associated type
//!   `type Output`; `DynamicOps` is not object-safe (its `convertTo` is
//!   generic), so the codec traits are parameterized by the *concrete ops
//!   type*: `Codec<A, Ops: DynamicOps>` (Java `Codec<A>` usable with any ops).
//! - Java `DataResult.Error`'s `Supplier<String>` message is stored eagerly;
//!   the suppliers in DFU are pure string concatenations, so eager evaluation
//!   is observationally equivalent.
//! - `Dynamic<O>` cannot retain its `DynamicOps` (STUB(mc.nbt): rivet-nbt
//!   constructs `Dynamic<Tag>` from a temporary ops reference); ops-dependent
//!   `Dynamic`/`DynamicLike` methods take the ops as a parameter.
//! - Java's anonymous-codec/`ComposerHolder` sharing is value-semantic here;
//!   codec combinators hold owned trait objects (`Box<dyn Codec<..>>`), so
//!   multi-argument functions are `Arc<dyn Fn>` to allow currying.

pub mod codec;
pub mod codecs;
pub mod data_result;
pub mod decoder;
pub mod dynamic;
pub mod dynamic_ops;
pub mod either;
pub mod encoder;
pub mod lifecycle;
pub mod map_codec;
pub mod map_decoder;
pub mod map_encoder;
pub mod optional_dynamic;
pub mod pair;
pub mod record_builder;
pub mod unit;

pub use codec::Codec;
pub use data_result::DataResult;
pub use decoder::Decoder;
pub use dynamic::Dynamic;
pub use dynamic_ops::{DynamicOps, ListBuilder, MapLike, RecordBuilder};
pub use either::Either;
pub use encoder::Encoder;
pub use lifecycle::Lifecycle;
pub use map_codec::MapCodec;
pub use map_decoder::MapDecoder;
pub use map_encoder::MapEncoder;
pub use optional_dynamic::OptionalDynamic;
pub use pair::Pair;
pub use record_builder::RecordCodecBuilder;
pub use unit::Unit;
