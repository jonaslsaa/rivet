//! `net.minecraft.world.level.storage` — world persistent data.
//!
//! #232 value slice: `LevelData` (and its `RespawnData` record) plus the
//! `PrimaryLevelData.parse` codec-cascade prerequisites (#54): `DataVersion`
//! and `LevelVersion` (the level.dat header block) and the `RespawnData`
//! codec wired on the `GlobalPos`/`BlockPos` map codecs.
//!
//! #398 trait surfaces: `WritableLevelData`, `ServerLevelData`, `WorldData`,
//! and `DerivedLevelData` — the value/interface layer the concrete
//! `PrimaryLevelData` (out of scope) implements. `LevelData` gains its
//! `fillCrashReportCategory` default and the `CrashReportCategory.formatLocation`
//! helper (`#398`). `WorldDataConfiguration`/`LevelSettings`/`FeatureFlagSet`
//! and `PrimaryLevelData.createTag` defer with sparse `RivetTodo` markers.

pub mod data_version;
pub mod derived_level_data;
pub mod level_data;
pub mod level_version;
pub mod server_level_data;
pub mod tag_value_input;
pub mod tag_value_output;
pub mod value_input;
pub mod value_input_context_helper;
pub mod value_output;
pub mod world_data;
pub mod writable_level_data;

pub use data_version::DataVersion;
pub use derived_level_data::DerivedLevelData;
pub use level_data::{LevelData, RespawnData, default_respawn_data, format_location};
pub use level_version::LevelVersion;
pub use server_level_data::ServerLevelData;
pub use tag_value_input::TagValueInput;
pub use tag_value_output::{SharedCompoundTag, TagValueOutput};
pub use value_input::{EmptyValueInput, TypedInputList, ValueInput, ValueInputList};
pub use value_input_context_helper::{TagContextOps, ValueInputContextHelper};
pub use value_output::{TypedOutputList, ValueOutput, ValueOutputList};
pub use world_data::WorldData;
pub use writable_level_data::WritableLevelData;
