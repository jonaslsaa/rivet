//! Paper `net.minecraft.world.MenuProvider`.

use rivet_text::Component;

use super::abstract_container_menu::AbstractContainerMenu;
use super::container::ContainerAccessContext;

/// Narrow Paper `net.minecraft.world.MenuProvider` seam for block entities and
/// server-side menu opening.
pub trait MenuProvider {
    /// `MenuProvider.getDisplayName()`.
    fn get_display_name(&self) -> Component;

    /// `MenuConstructor.createMenu(int, Inventory, Player)`.
    fn create_menu(
        &self,
        container_id: i32,
        ctx: &ContainerAccessContext,
    ) -> Option<AbstractContainerMenu>;
}
