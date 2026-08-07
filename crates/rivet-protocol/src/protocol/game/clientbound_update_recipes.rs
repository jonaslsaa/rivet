//! RivetTodo(#87): `ClientboundUpdateRecipesPacket` body not ported — the
//! `mc.network.protocol.game.join` slice is `done` except this body.
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
//! The join path sends it (capture id 133), but the real codec needs the full
//! recipe stack; it stays out of the byte-exact join send-set fixtures until
//! then. Discriminator: `packet_types::clientbound_update_recipes`.
