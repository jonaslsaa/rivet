//! Port of `ca.spottedleaf.dataconverter.types.ObjectType` — the uniform type
//! of a container element / map value.
//!
//! Java's `Class<?>` payload (`Byte.class`, `Number.class`, …) is replaced by
//! an enum discriminant since Rust has no runtime `Class`; `isNumber()` and
//! the `getType(Object)` dispatch are the only observable behavior, and both
//! survive exactly.

use crate::server::dataconverter::types::generic::Generic;

/// `ObjectType` — the 16 Java variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectType {
    None,
    Byte,
    Short,
    Int,
    Long,
    Float,
    Double,
    Number,
    ByteArray,
    ShortArray,
    IntArray,
    LongArray,
    List,
    Map,
    String,
    Undefined,
    Mixed,
}

impl ObjectType {
    /// `ObjectType.isNumber()` — `clazz != null && Number.class.isAssignableFrom(clazz)`.
    pub fn is_number(&self) -> bool {
        matches!(
            self,
            ObjectType::Byte
                | ObjectType::Short
                | ObjectType::Int
                | ObjectType::Long
                | ObjectType::Float
                | ObjectType::Double
                | ObjectType::Number
        )
    }

    /// `ObjectType.getType(Object)`.
    ///
    /// The Java `Number` branch returns `null` (not a dedicated variant) for an
    /// unhandled `Number` subtype, and the array branch does the same for an
    /// unhandled array class. `null` maps to `None` here.
    ///
    /// Java's `ObjectType.getType(null)` calls `object.getClass()` unguarded and
    /// throws NPE (ObjectType.java:59) — a faithful port panics rather than
    /// returning `None`. The probe `objectType.null_npe` records that behavior.
    pub fn get_type(object: &Generic) -> Option<ObjectType> {
        match object {
            Generic::Byte(_) => Some(ObjectType::Byte),
            Generic::Short(_) => Some(ObjectType::Short),
            Generic::Int(_) => Some(ObjectType::Int),
            Generic::Long(_) => Some(ObjectType::Long),
            Generic::Float(_) => Some(ObjectType::Float),
            Generic::Double(_) => Some(ObjectType::Double),
            Generic::Map(_) => Some(ObjectType::Map),
            Generic::List(_) => Some(ObjectType::List),
            Generic::Str(_) => Some(ObjectType::String),
            Generic::Bytes(_) => Some(ObjectType::ByteArray),
            Generic::Shorts(_) => Some(ObjectType::ShortArray),
            Generic::Ints(_) => Some(ObjectType::IntArray),
            Generic::Longs(_) => Some(ObjectType::LongArray),
            // `Boolean` is not a `Number`, a container, a `String`, or an array:
            // `getType(Boolean.TRUE)` returns null (probe `objectType.boolean`
            // is absent from the output — the JSON golden records no boolean
            // key, matching Java's null). It has no ObjectType.
            Generic::Bool(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::dataconverter::types::generic::Generic;

    #[test]
    fn number_variants_report_is_number() {
        assert!(ObjectType::Byte.is_number());
        assert!(ObjectType::Short.is_number());
        assert!(ObjectType::Int.is_number());
        assert!(ObjectType::Long.is_number());
        assert!(ObjectType::Float.is_number());
        assert!(ObjectType::Double.is_number());
        assert!(ObjectType::Number.is_number());
        assert!(!ObjectType::None.is_number());
        assert!(!ObjectType::String.is_number());
        assert!(!ObjectType::Map.is_number());
        assert!(!ObjectType::Undefined.is_number());
        assert!(!ObjectType::Mixed.is_number());
    }

    #[test]
    fn get_type_dispatch_matches_probe() {
        assert_eq!(
            ObjectType::get_type(&Generic::Byte(3)),
            Some(ObjectType::Byte)
        );
        assert_eq!(
            ObjectType::get_type(&Generic::Short(3)),
            Some(ObjectType::Short)
        );
        assert_eq!(
            ObjectType::get_type(&Generic::Int(3)),
            Some(ObjectType::Int)
        );
        assert_eq!(
            ObjectType::get_type(&Generic::Long(3)),
            Some(ObjectType::Long)
        );
        assert_eq!(
            ObjectType::get_type(&Generic::Float(3.0)),
            Some(ObjectType::Float)
        );
        assert_eq!(
            ObjectType::get_type(&Generic::Double(3.0)),
            Some(ObjectType::Double)
        );
        assert_eq!(
            ObjectType::get_type(&Generic::Str("abc".into())),
            Some(ObjectType::String)
        );
        assert_eq!(
            ObjectType::get_type(&Generic::Bytes(vec![1, 2])),
            Some(ObjectType::ByteArray)
        );
        assert_eq!(
            ObjectType::get_type(&Generic::Shorts(vec![1, 2])),
            Some(ObjectType::ShortArray)
        );
        assert_eq!(
            ObjectType::get_type(&Generic::Ints(vec![1, 2])),
            Some(ObjectType::IntArray)
        );
        assert_eq!(
            ObjectType::get_type(&Generic::Longs(vec![1, 2])),
            Some(ObjectType::LongArray)
        );
        // Boolean and BigDecimal are not one of the Java Number subtypes the
        // dispatch recognizes — the Java code falls through to null (None).
        assert_eq!(ObjectType::get_type(&Generic::Bool(true)), None);
    }
}
