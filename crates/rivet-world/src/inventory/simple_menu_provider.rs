//! Paper `net.minecraft.world.SimpleMenuProvider`.

use rivet_text::Component;

use super::abstract_container_menu::AbstractContainerMenu;
use super::container::ContainerAccessContext;
use super::menu_provider::MenuProvider;

/// Paper `net.minecraft.world.SimpleMenuProvider`.
pub struct SimpleMenuProvider<F>
where
    F: Fn(i32, &ContainerAccessContext) -> Option<AbstractContainerMenu>,
{
    title: Component,
    constructor: F,
}

impl<F> SimpleMenuProvider<F>
where
    F: Fn(i32, &ContainerAccessContext) -> Option<AbstractContainerMenu>,
{
    pub fn new(title: Component, constructor: F) -> Self {
        Self { title, constructor }
    }
}

impl<F> MenuProvider for SimpleMenuProvider<F>
where
    F: Fn(i32, &ContainerAccessContext) -> Option<AbstractContainerMenu>,
{
    fn get_display_name(&self) -> Component {
        self.title.clone()
    }

    fn create_menu(
        &self,
        container_id: i32,
        ctx: &ContainerAccessContext,
    ) -> Option<AbstractContainerMenu> {
        (self.constructor)(container_id, ctx)
    }
}
