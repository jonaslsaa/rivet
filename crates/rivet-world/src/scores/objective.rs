//! Port of `net.minecraft.world.scores.Objective` (Paper 26.2).

use crate::scores::{
    NumberFormat, ObjectiveCriteria, RenderType, show_text_hover, wrap_in_square_brackets,
};
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use rivet_text::{Component, Style, component_serialization};
use std::sync::Arc;

/// Port of `net.minecraft.world.scores.Objective`.
///
/// Mutations that must notify the owning `Scoreboard` / `ServerScoreboard`
/// go through `ServerScoreboard`'s objective mutators so packet emission and
/// dirty tracking are always authoritative.
#[derive(Clone, Debug, PartialEq)]
pub struct Objective {
    name: String,
    criteria: ObjectiveCriteria,
    display_name: Component,
    formatted_display_name: Component,
    render_type: RenderType,
    display_auto_update: bool,
    number_format: Option<NumberFormat>,
}

impl Objective {
    /// Construct an `Objective` matching `new Objective(scoreboard, name, criteria, displayName, renderType, displayAutoUpdate, numberFormat)`.
    pub fn new(
        name: impl Into<String>,
        criteria: ObjectiveCriteria,
        display_name: Component,
        render_type: RenderType,
        display_auto_update: bool,
        number_format: Option<NumberFormat>,
    ) -> Self {
        let name = name.into();
        let formatted_display_name = create_formatted_display_name(&name, &display_name);
        Self {
            name,
            criteria,
            display_name,
            formatted_display_name,
            render_type,
            display_auto_update,
            number_format,
        }
    }

    /// `Objective.pack()`.
    pub fn pack(&self) -> PackedObjective {
        PackedObjective {
            name: self.name.clone(),
            criteria: self.criteria.clone(),
            display_name: self.display_name.clone(),
            render_type: self.render_type,
            display_auto_update: self.display_auto_update,
            number_format: self.number_format.clone(),
        }
    }

    #[inline]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[inline]
    pub fn criteria(&self) -> &ObjectiveCriteria {
        &self.criteria
    }

    #[inline]
    pub fn display_name(&self) -> &Component {
        &self.display_name
    }

    #[inline]
    pub fn formatted_display_name(&self) -> &Component {
        &self.formatted_display_name
    }

    #[inline]
    pub fn render_type(&self) -> RenderType {
        self.render_type
    }

    #[inline]
    pub fn display_auto_update(&self) -> bool {
        self.display_auto_update
    }

    #[inline]
    pub fn number_format(&self) -> Option<&NumberFormat> {
        self.number_format.as_ref()
    }

    /// `Objective.numberFormatOrDefault(NumberFormat)`.
    pub fn number_format_or_default<'a>(
        &'a self,
        default_format: &'a NumberFormat,
    ) -> &'a NumberFormat {
        self.number_format.as_ref().unwrap_or(default_format)
    }

    pub(crate) fn set_display_name_raw(&mut self, display_name: Component) {
        self.display_name = display_name;
        self.formatted_display_name = create_formatted_display_name(&self.name, &self.display_name);
    }

    pub(crate) fn set_render_type_raw(&mut self, render_type: RenderType) {
        self.render_type = render_type;
    }

    pub(crate) fn set_display_auto_update_raw(&mut self, display_auto_update: bool) {
        self.display_auto_update = display_auto_update;
    }

    pub(crate) fn set_number_format_raw(&mut self, number_format: Option<NumberFormat>) {
        self.number_format = number_format;
    }
}

/// `ComponentUtils.wrapInSquareBrackets(displayName.copy().withStyle(s -> s.withHoverEvent(ShowText(literal(name)))))`.
fn create_formatted_display_name(name: &str, display_name: &Component) -> Component {
    let styled = display_name
        .copy()
        .with_style(Style::EMPTY.with_hover_event(Some(show_text_hover(Component::literal(name)))));
    wrap_in_square_brackets(styled)
}

/// Port of `net.minecraft.world.scores.Objective.Packed`.
#[derive(Clone, Debug, PartialEq)]
pub struct PackedObjective {
    pub name: String,
    pub criteria: ObjectiveCriteria,
    pub display_name: Component,
    pub render_type: RenderType,
    pub display_auto_update: bool,
    pub number_format: Option<NumberFormat>,
}

impl PackedObjective {
    /// `Objective.Packed.CODEC`.
    ///
    /// Exact Paper field names and defaults:
    /// - `"Name"` (required)
    /// - `"CriteriaName"` (default `ObjectiveCriteria.DUMMY`)
    /// - `"DisplayName"` (required `Component`)
    /// - `"RenderType"` (default `RenderType.INTEGER`)
    /// - `"display_auto_update"` (default `false`)
    /// - `"format"` (optional `NumberFormat`)
    pub fn codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<PackedObjective, Ops>> {
        let comp_codec = component_serialization::codec::<Ops>();
        let nf_codec = NumberFormat::codec::<Ops>();

        record_builder::create(|instance| {
            instance
                .group(RecordCodecBuilder::of(
                    Arc::new(|p: &PackedObjective| p.name.clone()),
                    codec::field_of(codec::string_codec::<Ops>(), "Name".to_owned()),
                ))
                .and(RecordCodecBuilder::of(
                    Arc::new(|p: &PackedObjective| p.criteria.clone()),
                    codec::optional_field_of(
                        "CriteriaName",
                        ObjectiveCriteria::codec::<Ops>(),
                        ObjectiveCriteria::dummy(),
                    ),
                ))
                .and(RecordCodecBuilder::of(
                    Arc::new(|p: &PackedObjective| p.display_name.clone()),
                    codec::field_of(comp_codec, "DisplayName".to_owned()),
                ))
                .and(RecordCodecBuilder::of(
                    Arc::new(|p: &PackedObjective| p.render_type),
                    codec::optional_field_of(
                        "RenderType",
                        RenderType::codec::<Ops>(),
                        RenderType::Integer,
                    ),
                ))
                .and(RecordCodecBuilder::of(
                    Arc::new(|p: &PackedObjective| p.display_auto_update),
                    codec::optional_field_of(
                        "display_auto_update",
                        codec::bool_codec::<Ops>(),
                        false,
                    ),
                ))
                .and(RecordCodecBuilder::of(
                    Arc::new(|p: &PackedObjective| p.number_format.clone()),
                    codec::optional_field("format".to_owned(), nf_codec, false),
                ))
                .apply(
                    instance,
                    Arc::new(
                        |name,
                         criteria,
                         display_name,
                         render_type,
                         display_auto_update,
                         number_format| PackedObjective {
                            name,
                            criteria,
                            display_name,
                            render_type,
                            display_auto_update,
                            number_format,
                        },
                    ),
                )
        })
    }
}

impl rivet_serialization::codec::JavaEquals for PackedObjective {
    fn java_equals(&self, other: &Self) -> bool {
        self == other
    }
}
