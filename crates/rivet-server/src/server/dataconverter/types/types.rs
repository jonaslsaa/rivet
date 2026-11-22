//! Concrete [`TypeUtil`](super::type_util::TypeUtil) singletons.
//!
//! Mirrors `ca.spottedleaf.dataconverter.types.Types` from Paper 26.2.

use crate::server::dataconverter::types::json::JsonTypeUtil;
use crate::server::dataconverter::types::nbt::NBTTypeUtil;

/// Singleton handles for the concrete DataConverter backing types.
pub struct Types;

impl Types {
    pub const NBT: NBTTypeUtil = NBTTypeUtil;

    pub const JSON: JsonTypeUtil = JsonTypeUtil::new(false);

    pub const JSON_COMPRESSED: JsonTypeUtil = JsonTypeUtil::new(true);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::dataconverter::types::generic::Generic;
    use crate::server::dataconverter::types::list_type::ListType;
    use crate::server::dataconverter::types::map_type::MapType;
    use crate::server::dataconverter::types::nbt::{NBTListType, NBTMapType};
    use crate::server::dataconverter::types::type_util::{TypeUtil, TypeUtilBase};

    #[test]
    fn cross_format_conversion_nbt_and_json() {
        let mut nbt_map = NBTMapType::new();
        nbt_map.set_string("name", "minecraft:stone".to_owned());
        nbt_map.set_int("count", 64);
        nbt_map.set_boolean("flag", true);
        let mut sub_list = NBTListType::new();
        sub_list.add_string("a".to_owned());
        sub_list.add_string("b".to_owned());
        nbt_map.set_list("tags", Box::new(sub_list));

        // NBT -> JSON
        let json_generic = Types::NBT
            .convert_to(Some(&Generic::Map(Box::new(nbt_map))), &Types::JSON)
            .expect("converted to json");
        let Generic::Map(json_map) = json_generic else {
            panic!("expected json map");
        };
        assert_eq!(
            json_map.get_string("name").as_deref(),
            Some("minecraft:stone")
        );
        assert_eq!(json_map.get_int("count"), 64);
        assert!(json_map.get_boolean("flag"));
        let tags = json_map.get_list_unchecked("tags").expect("tags list");
        assert_eq!(tags.size(), 2);
        assert_eq!(tags.get_string(0), "a");
        assert_eq!(tags.get_string(1), "b");

        // JSON -> NBT
        let back_to_nbt = Types::JSON
            .convert_to(Some(&Generic::Map(json_map)), &Types::NBT)
            .expect("converted back to nbt");
        let Generic::Map(nbt_back) = back_to_nbt else {
            panic!("expected nbt map");
        };
        assert_eq!(
            nbt_back.get_string("name").as_deref(),
            Some("minecraft:stone")
        );
        assert_eq!(nbt_back.get_int("count"), 64);
        assert_eq!(nbt_back.get_byte("flag"), 1);

        // JSON_COMPRESSED creates compressed maps
        let mut compressed = Types::JSON_COMPRESSED.create_empty_map();
        compressed.set_string("n", "42".to_owned());
        assert_eq!(compressed.get_int("n"), 42);
    }
}
