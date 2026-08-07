//! STUB(mc.network.protocol.game.join) — `ClientboundUpdateRecipesPacket` body
//! not ported.
//!
//! Java: `ClientboundUpdateRecipesPacket.java` in `working/Paper`. Carries a
//! `Map<ResourceKey<RecipePropertySet>, RecipePropertySet> itemSets` (codec
//! `RecipePropertySet.STREAM_CODEC` = `Item.STREAM_CODEC.apply(ByteBufCodecs
//! .list())`) and a `SelectableRecipe.SingleInputSet<StonecutterRecipe>` (codec
//! `Ingredient.CONTENTS_STREAM_CODEC` + `SlotDisplay.STREAM_CODEC`).
//!
//! BLOCKED on the recipe-registry value types (`world.item.crafting` —
//! `Item`/`Ingredient`/`SlotDisplay`/`RecipePropertySet`/`SelectableRecipe`,
//! not yet ported; epic #12 text/NBT + world-item units).
//! RivetTodo(#87): the `update_recipes` body is deferred with the
//! `world.item.crafting` unit. The join path sends it (capture id 133), but the
//! real codec needs the full recipe stack; it stays out of the byte-exact join
//! send-set fixtures until then. Discriminator:
//! `packet_types::clientbound_update_recipes`.
