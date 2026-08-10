//! `net.minecraft.world.level.storage` — world persistent data.
//!
//! #232 value slice: only `LevelData` (and its `RespawnData` record) is in
//! this unit's file list. `WritableLevelData` and `ServerLevelData` are
//! separate `storage` files deferred with the concrete world data (issue #232
//! tracks the full `mc.world.level` unit).
//!
//! This slice adds the `PrimaryLevelData.parse` codec-cascade prerequisites
//! (issue #54): `DataVersion` and `LevelVersion` (the level.dat header block),
//! plus the `RespawnData` codec wired on the `GlobalPos`/`BlockPos` map codecs.

pub mod data_version;
pub mod level_data;
pub mod level_version;
pub mod tag_value_input;
pub mod tag_value_output;
pub mod value_input;
pub mod value_input_context_helper;
pub mod value_output;

pub use data_version::DataVersion;
pub use level_data::{LevelData, RespawnData, default_respawn_data};
pub use level_version::LevelVersion;
pub use tag_value_input::TagValueInput;
pub use tag_value_output::TagValueOutput;
pub use value_input::{EmptyValueInput, TypedInputList, ValueInput, ValueInputList};
pub use value_input_context_helper::{TagContextOps, ValueInputContextHelper};
pub use value_output::{TypedOutputList, ValueOutput, ValueOutputList};
