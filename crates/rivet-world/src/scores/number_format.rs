//! Port of `net.minecraft.network.chat.numbers.{NumberFormat, NumberFormatTypes, BlankFormat, StyledFormat, FixedFormat}` (Paper 26.2).

use crate::scores::data_result_to_result;
use rivet_core::ChatFormatting;
use rivet_nbt::nbt_accounter::NbtAccounter;
use rivet_nbt::nbt_ops::NbtOps;
use rivet_nbt::tag::Tag;
use rivet_protocol::chat::trusted_component_with_registries;
use rivet_protocol::codec::{CodecError, StreamDecoder, StreamEncoder};
use rivet_protocol::registry_friendly_byte_buf::RegistryFriendlyByteBuf;
use rivet_registry::registry_ops::RegistryOps;
use rivet_serialization::codec::Codec;
use rivet_serialization::data_result::DataResult;
use rivet_serialization::decoder::Decoder;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::encoder::Encoder;
use rivet_serialization::map_codec::{self, MapCodec};
use rivet_text::{Component, Style, component_serialization, style};
use std::fmt;
use std::sync::{Arc, OnceLock};

/// Built-in `NumberFormatType` registry IDs (`NumberFormatTypes.bootstrap`).
pub const BLANK_TYPE_ID: i32 = 0;
pub const STYLED_TYPE_ID: i32 = 1;
pub const FIXED_TYPE_ID: i32 = 2;

/// Port of `net.minecraft.network.chat.numbers.NumberFormat`.
#[derive(Clone, Debug, PartialEq)]
pub enum NumberFormat {
    /// `BlankFormat.INSTANCE` — renders scores as an empty component.
    Blank,
    /// `StyledFormat(Style)` — renders the decimal score with a `Style`.
    Styled(Style),
    /// `FixedFormat(Component)` — renders a fixed component regardless of value.
    Fixed(Component),
}

impl NumberFormat {
    /// `StyledFormat.NO_STYLE`.
    pub fn no_style() -> Self {
        NumberFormat::Styled(Style::EMPTY)
    }

    /// `StyledFormat.SIDEBAR_DEFAULT` (red).
    pub fn sidebar_default() -> Self {
        NumberFormat::Styled(Style::EMPTY.with_color_format(ChatFormatting::Red))
    }

    /// `StyledFormat.PLAYER_LIST_DEFAULT` (yellow).
    pub fn player_list_default() -> Self {
        NumberFormat::Styled(Style::EMPTY.with_color_format(ChatFormatting::Yellow))
    }

    /// `NumberFormat.format(int value)`.
    pub fn format(&self, value: i32) -> Component {
        match self {
            NumberFormat::Blank => Component::empty(),
            NumberFormat::Styled(style) => {
                Component::literal(&value.to_string()).with_style(style.clone())
            }
            NumberFormat::Fixed(component) => component.copy(),
        }
    }

    /// Registry numeric ID (`0 = blank`, `1 = styled`, `2 = fixed`).
    pub const fn type_id(&self) -> i32 {
        match self {
            NumberFormat::Blank => BLANK_TYPE_ID,
            NumberFormat::Styled(_) => STYLED_TYPE_ID,
            NumberFormat::Fixed(_) => FIXED_TYPE_ID,
        }
    }

    /// Canonical registry name (`"minecraft:blank"`, `"minecraft:styled"`, `"minecraft:fixed"`).
    pub const fn type_name(&self) -> &'static str {
        match self {
            NumberFormat::Blank => "minecraft:blank",
            NumberFormat::Styled(_) => "minecraft:styled",
            NumberFormat::Fixed(_) => "minecraft:fixed",
        }
    }

    /// `NumberFormatTypes.CODEC` over any `DynamicOps`.
    pub fn codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<NumberFormat, Ops>> {
        Arc::new(NumberFormatCodec::<Ops>::new())
    }

    /// Encode on the wire (`NumberFormatTypes.STREAM_CODEC`).
    pub fn encode_stream(&self, output: &mut RegistryFriendlyByteBuf) -> Result<(), CodecError> {
        output.inner_mut().write_var_int(self.type_id());
        match self {
            NumberFormat::Blank => Ok(()),
            NumberFormat::Styled(style) => {
                let ops =
                    RegistryOps::create_from_access(&NbtOps, output.registry_access().clone());
                let tag =
                    data_result_to_result(style_codec_with_registries().encode_start(&ops, style))
                        .map_err(|err| CodecError::new(format!("Failed to encode: {err}")))?;
                output.inner_mut().write_nbt(Some(&tag));
                Ok(())
            }
            NumberFormat::Fixed(component) => {
                trusted_component_with_registries().encode(output, component)
            }
        }
    }

    /// Decode from the wire (`NumberFormatTypes.STREAM_CODEC`).
    pub fn decode_stream(input: &mut RegistryFriendlyByteBuf) -> Result<Self, CodecError> {
        let type_id = input.inner_mut().read_var_int();
        match type_id {
            BLANK_TYPE_ID => Ok(NumberFormat::Blank),
            STYLED_TYPE_ID => {
                let tag = input
                    .inner_mut()
                    .read_nbt_with_accounter(&mut NbtAccounter::unlimited_heap())
                    .ok_or_else(|| CodecError::new("Failed to decode: missing style NBT tag"))?;
                let ops = RegistryOps::create_from_access(&NbtOps, input.registry_access().clone());
                let style = data_result_to_result(style_codec_with_registries().parse(&ops, &tag))
                    .map_err(|err| CodecError::new(format!("Failed to decode: {err}")))?;
                Ok(NumberFormat::Styled(style))
            }
            FIXED_TYPE_ID => {
                let component = trusted_component_with_registries().decode(input)?;
                Ok(NumberFormat::Fixed(component))
            }
            other => Err(CodecError::new(format!(
                "Unknown number format type id: {other}"
            ))),
        }
    }

    /// Encode `Optional<NumberFormat>` (`NumberFormatTypes.OPTIONAL_STREAM_CODEC`).
    pub fn encode_optional_stream(
        value: &Option<NumberFormat>,
        output: &mut RegistryFriendlyByteBuf,
    ) -> Result<(), CodecError> {
        match value {
            Some(fmt) => {
                output.inner_mut().write_boolean(true);
                fmt.encode_stream(output)
            }
            None => {
                output.inner_mut().write_boolean(false);
                Ok(())
            }
        }
    }

    /// Decode `Optional<NumberFormat>` (`NumberFormatTypes.OPTIONAL_STREAM_CODEC`).
    pub fn decode_optional_stream(
        input: &mut RegistryFriendlyByteBuf,
    ) -> Result<Option<NumberFormat>, CodecError> {
        if input.inner_mut().read_boolean() {
            Ok(Some(Self::decode_stream(input)?))
        } else {
            Ok(None)
        }
    }
}

fn style_codec_with_registries() -> Arc<dyn Codec<Style, RegistryOps<Tag, NbtOps>>> {
    static CODEC: OnceLock<Arc<dyn Codec<Style, RegistryOps<Tag, NbtOps>>>> = OnceLock::new();
    CODEC
        .get_or_init(|| {
            let component = component_serialization::codec::<RegistryOps<Tag, NbtOps>>();
            map_codec::codec_of(style::serializer::map_codec::<RegistryOps<Tag, NbtOps>>(
                component,
            ))
        })
        .clone()
}

struct NumberFormatCodec<Ops: DynamicOps + 'static> {
    component_codec: Arc<dyn Codec<Component, Ops>>,
    style_map_codec: Arc<dyn MapCodec<Style, Ops>>,
}

impl<Ops: DynamicOps + 'static> fmt::Debug for NumberFormatCodec<Ops> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("NumberFormatCodec")
    }
}

impl<Ops: DynamicOps + 'static> NumberFormatCodec<Ops> {
    fn new() -> Self {
        let component_codec = component_serialization::codec::<Ops>();
        let style_map_codec = style::serializer::map_codec::<Ops>(component_codec.clone());
        Self {
            component_codec,
            style_map_codec,
        }
    }
}

impl<Ops: DynamicOps + 'static> Encoder<NumberFormat, Ops> for NumberFormatCodec<Ops> {
    fn encode(
        &self,
        input: &NumberFormat,
        ops: &Ops,
        prefix: &Ops::Output,
    ) -> DataResult<Ops::Output> {
        let mut builder = ops.map_builder();
        let type_key = ops.create_string("type".to_owned());
        let type_val = ops.create_string(input.type_name().to_owned());
        builder.add(type_key, type_val);

        match input {
            NumberFormat::Blank => {}
            NumberFormat::Styled(style) => {
                self.style_map_codec.encode(style, ops, builder.as_mut());
            }
            NumberFormat::Fixed(component) => {
                let encoded = self.component_codec.encode_start(ops, component);
                builder.add_result(ops.create_string("value".to_owned()), encoded);
            }
        }

        builder.build(Some(prefix.clone()))
    }
}

impl<Ops: DynamicOps + 'static> Decoder<NumberFormat, Ops> for NumberFormatCodec<Ops> {
    fn decode(&self, ops: &Ops, input: &Ops::Output) -> DataResult<(NumberFormat, Ops::Output)> {
        let map = match data_result_to_result(ops.get_map(input)) {
            Ok(m) => m,
            Err(e) => return DataResult::error(e),
        };
        let type_raw = match map.get_string("type") {
            Some(v) => v,
            None => return DataResult::error("Missing 'type' field in NumberFormat".to_owned()),
        };
        let type_name = match data_result_to_result(ops.get_string_value(&type_raw)) {
            Ok(s) => s,
            Err(e) => return DataResult::error(e),
        };
        let normalized = type_name.strip_prefix("minecraft:").unwrap_or(&type_name);
        let parsed = match normalized {
            "blank" => DataResult::success(NumberFormat::Blank),
            "styled" => self
                .style_map_codec
                .decode(ops, map.as_ref())
                .map_owned(NumberFormat::Styled),
            "fixed" => match map.get_string("value") {
                Some(val) => self
                    .component_codec
                    .parse(ops, &val)
                    .map_owned(NumberFormat::Fixed),
                None => DataResult::error("Missing 'value' field in FixedFormat".to_owned()),
            },
            other => DataResult::error(format!("Unknown number format type: {other}")),
        };
        parsed.map_owned(|nf| (nf, ops.empty()))
    }
}

impl<Ops: DynamicOps + 'static> Codec<NumberFormat, Ops> for NumberFormatCodec<Ops> {}
