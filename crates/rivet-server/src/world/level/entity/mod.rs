//! `net.minecraft.world.level.entity`: entity lookup, section storage, visibility and the
//! persistent entity section manager (Paper 26.2).

pub mod chunk_entities;
pub mod entity_lookup;
pub mod entity_section;
pub mod persistent_entity_section_manager;
pub mod visibility;

#[cfg(test)]
mod tests;
