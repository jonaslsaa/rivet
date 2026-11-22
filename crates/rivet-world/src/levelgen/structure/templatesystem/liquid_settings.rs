//! Port of `net.minecraft.world.level.levelgen.structure.templatesystem.LiquidSettings`
//! (enum, 26.2).
//!
//! Controls whether waterlogged blocks in a structure template apply their
//! waterlogging when placed into the world.

use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_util::string_representable::{self, EnumCodec, EnumOrdinal, StringRepresentable};

/// `net.minecraft.world.level.levelgen.structure.templatesystem.LiquidSettings`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum LiquidSettings {
    /// `IGNORE_WATERLOGGING("ignore_waterlogging")`
    IgnoreWaterlogging,
    /// `APPLY_WATERLOGGING("apply_waterlogging")`
    ApplyWaterlogging,
}

impl LiquidSettings {
    /// Declaration order matching `LiquidSettings.values()`.
    pub const VALUES: [LiquidSettings; 2] = [
        LiquidSettings::IgnoreWaterlogging,
        LiquidSettings::ApplyWaterlogging,
    ];
}

impl StringRepresentable for LiquidSettings {
    fn get_serialized_name(&self) -> &str {
        match self {
            LiquidSettings::IgnoreWaterlogging => "ignore_waterlogging",
            LiquidSettings::ApplyWaterlogging => "apply_waterlogging",
        }
    }
}

impl EnumOrdinal for LiquidSettings {
    fn ordinal(&self) -> usize {
        match self {
            LiquidSettings::IgnoreWaterlogging => 0,
            LiquidSettings::ApplyWaterlogging => 1,
        }
    }
}

impl std::fmt::Display for LiquidSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.get_serialized_name())
    }
}

/// `LiquidSettings.CODEC` — `StringRepresentable.fromValues(LiquidSettings::values)`.
pub fn liquid_settings_codec<Ops: DynamicOps + 'static>() -> EnumCodec<LiquidSettings, Ops> {
    string_representable::from_enum::<LiquidSettings, Ops>(&LiquidSettings::VALUES)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_serialization::decoder::Decoder;
    use rivet_serialization::encoder::Encoder;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    #[test]
    fn codec_round_trips_both_variants() {
        let codec = liquid_settings_codec::<JsonOps>();
        for (variant, expected) in [
            (LiquidSettings::IgnoreWaterlogging, "ignore_waterlogging"),
            (LiquidSettings::ApplyWaterlogging, "apply_waterlogging"),
        ] {
            let encoded = codec
                .encode_start(&JsonOps::INSTANCE, &variant)
                .result()
                .expect("encode should succeed")
                .clone();
            assert_eq!(encoded, json!(expected));
            let decoded = *codec
                .parse(&JsonOps::INSTANCE, &encoded)
                .result()
                .expect("decode should succeed");
            assert_eq!(decoded, variant);
        }
    }

    #[test]
    fn codec_rejects_unknown_value() {
        let codec = liquid_settings_codec::<JsonOps>();
        let res = codec.parse(&JsonOps::INSTANCE, &json!("unknown_liquid"));
        assert!(res.is_error());
    }
}
