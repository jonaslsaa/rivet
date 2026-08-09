//! Port of `net.minecraft.server.players.NameAndId` (issue #86) — the value
//! type + CODEC only.
//!
//! Java: `NameAndId.java` in `working/Paper`. `(UUID id, String name)` with
//! `CODEC = record { "id": UUIDUtil.STRING_CODEC, "name": Codec.STRING }`.
//! `ServerStatus.Players.sample` is a `List<NameAndId>`, so this CODEC is a
//! status-protocol dependency.
//!
//! RECONCILIATION: `NameAndId` is owned by the `mc.server.players` manifest
//! unit (`rivet-server`), which is not ported yet; this leaf hosts it in
//! `protocol::status` because `rivet-protocol` cannot depend on `rivet-server`
//! (the dependency runs the other way). When `mc.server.players` lands, the
//! type moves to `rivet-server`'s `server::players` and this module re-exports
//! or drops it (the CODEC is ops-generic, so the move is mechanical). Only the
//! `UUIDUtil.STRING_CODEC`-backed surface is ported: the `GameProfile`
//! constructors, `fromJson`/`appendTo`, and `createOffline` need authlib/JSON
//! surface not in this slice.
//!
//! `UUIDUtil.STRING_CODEC` is `Codec.STRING.comapFlatMap(UUID::fromString,
//! UUID::toString)` — a string codec mapped through `Uuid::from_string`, which
//! replicates `UUID.fromString`'s accept/reject set exactly (dashed forms only,
//! at most 36 chars; braces/`urn:uuid:`/undashed are rejected; a group whose
//! value exceeds `Long.MAX_VALUE` is rejected too) and surfaces decode failures
//! as Java's `"Invalid UUID <s>: <cause>"` error, with the thrown
//! `IllegalArgumentException`/`NumberFormatException` message appended verbatim.

use rivet_serialization::codec::{self, Codec};
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec;
use rivet_util::uuid::Uuid;
use std::sync::Arc;

/// `net.minecraft.server.players.NameAndId` — `(UUID id, String name)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NameAndId {
    id: Uuid,
    name: String,
}

impl NameAndId {
    /// `new NameAndId(UUID id, String name)`.
    pub fn new(id: Uuid, name: String) -> Self {
        NameAndId { id, name }
    }

    /// `NameAndId.id()`.
    pub fn id(&self) -> Uuid {
        self.id
    }

    /// `NameAndId.name()`.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// `NameAndId.CODEC` — `record { "id": UUIDUtil.STRING_CODEC, "name":
    /// Codec.STRING }` via `RecordCodecBuilder`.
    pub fn codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<NameAndId, Ops>> {
        rivet_serialization::record_builder::create(move |instance| {
            let id = map_codec::for_getter(
                codec::field_of(uuid_string_codec::<Ops>(), "id".to_string()),
                Arc::new(|v: &NameAndId| v.id),
            );
            let name = map_codec::for_getter(
                codec::field_of(codec::string_codec::<Ops>(), "name".to_string()),
                Arc::new(|v: &NameAndId| v.name.clone()),
            );
            instance.group(id).and(name).apply(
                instance,
                Arc::new(|id: Uuid, name: String| NameAndId::new(id, name)),
            )
        })
    }
}

/// `UUIDUtil.STRING_CODEC` — `Codec.STRING.comapFlatMap(UUID::fromString,
/// UUID::toString)`. On decode the string is parsed via `Uuid::from_string`
/// (`UUID.fromString`, which throws for a malformed UUID) and a failure is
/// surfaced as `DataResult.error` with Java's exact
/// `"Invalid UUID <s>: <cause>"` message — the cause is the thrown
/// `IllegalArgumentException`/`NumberFormatException` message, appended
/// verbatim; on encode the canonical `UUID.toString()` form.
fn uuid_string_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<Uuid, Ops>> {
    codec::comap_flat_map(
        codec::string_codec::<Ops>(),
        Arc::new(|s: &String| match Uuid::from_string(s) {
            Ok(uuid) => DataResult::success(uuid),
            Err(cause) => DataResult::error(format!("Invalid UUID {s}: {cause}")),
        }),
        Arc::new(|uuid: &Uuid| uuid.to_string()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_serialization::json_ops::JsonOps;

    fn uuid(most: u64, least: u64) -> Uuid {
        Uuid {
            most: most as i64,
            least: least as i64,
        }
    }

    #[test]
    fn from_string_matches_java_accept_set() {
        // Java `UUID.fromString` (verified on the JDK 25 oracle JVM).
        let canonical = "00112233-4455-6677-8899-aabbccddeeff";
        assert_eq!(
            Uuid::from_string(canonical),
            Ok(uuid(0x00112233_44556677, 0x8899_aabbccddeeff))
        );
        // Uppercase hex is accepted.
        assert_eq!(
            Uuid::from_string("FFFFFFFF-FFFF-FFFF-FFFF-FFFFFFFFFFFF"),
            Ok(uuid(0xffffffff_ffffffff, 0xffff_ffffffffffff))
        );
        // Variable-width dashed groups pad with zeros (`fromString1`).
        assert_eq!(
            Uuid::from_string("1-2-3-4-5"),
            Ok(uuid(0x00000001_00020003, 0x0004_000000000005))
        );
        // `Long.parseLong` accepts a leading `+` in a group.
        assert_eq!(
            Uuid::from_string("1-+2-3-4-5"),
            Ok(uuid(0x00000001_00020003, 0x0004_000000000005))
        );
        // A group wider than its field truncates via the mask (Java
        // `& 0x...`), as long as the raw value fits a signed long.
        assert_eq!(
            Uuid::from_string("100000000-2-3-4-5"),
            Ok(uuid(0x00000000_00020003, 0x0004_000000000005))
        );
        // The all-zero UUID parses fine (no variant validation).
        assert_eq!(
            Uuid::from_string("00000000-0000-0000-0000-000000000000"),
            Ok(uuid(0, 0))
        );
    }

    #[test]
    fn from_string_rejects_java_rejected_forms() {
        // >36 chars is `IllegalArgumentException("UUID string too large")`;
        // braces, `urn:uuid:`, and a trailing dash all exceed 36 chars.
        assert_eq!(
            Uuid::from_string("{00112233-4455-6677-8899-aabbccddeeff}"),
            Err("UUID string too large".to_string())
        );
        assert_eq!(
            Uuid::from_string("urn:uuid:00112233-4455-6677-8899-aabbccddeeff"),
            Err("UUID string too large".to_string())
        );
        assert_eq!(
            Uuid::from_string("00112233-4455-6677-8899-aabbccddeefff"),
            Err("UUID string too large".to_string())
        );
        assert_eq!(
            Uuid::from_string("00112233-4455-6677-8899-aabbccddeeff-"),
            Err("UUID string too large".to_string())
        );
        // The undashed 32-char form has no 4th dash.
        assert_eq!(
            Uuid::from_string("00112233445566778899aabbccddeeff"),
            Err("Invalid UUID string: 00112233445566778899aabbccddeeff".to_string())
        );
        // A 5th dash (≤36 chars) is "Invalid UUID string".
        assert_eq!(
            Uuid::from_string("1-2-3-4-5-6"),
            Err("Invalid UUID string: 1-2-3-4-5-6".to_string())
        );
        // Empty groups: `Long.parseLong("", 16)`.
        assert_eq!(
            Uuid::from_string(""),
            Err("Invalid UUID string: ".to_string())
        );
        assert_eq!(
            Uuid::from_string("00112233--6677-8899-aabbccddeeff"),
            Err("For input string: \"\" under radix 16".to_string())
        );
        // Non-hex digits report Java's error index within the failing segment.
        assert_eq!(
            Uuid::from_string("0011223g-4455-6677-8899-aabbccddeeff"),
            Err("Error at index 7 in: \"0011223g\"".to_string())
        );
        assert_eq!(
            Uuid::from_string("00112233-4455-6677-8899-aabbccddeefg"),
            Err("Error at index 11 in: \"aabbccddeefg\"".to_string())
        );
    }

    #[test]
    fn from_string_rejects_groups_above_long_max() {
        // A 16-hex-digit group starting `8`..`f` exceeds `Long.MAX_VALUE`
        // (`0x7fffffffffffffff`), so Java's `Long.parseLong(segment, 16)`
        // overflows at the last digit (index 15) and `UUID.fromString`
        // rejects the whole string. The old unsigned parse wrongly accepted
        // these (masked to 0).
        assert_eq!(
            Uuid::from_string("8000000000000000-a-b-c-d"),
            Err("Error at index 15 in: \"8000000000000000\"".to_string())
        );
        assert_eq!(
            Uuid::from_string("ffffffffffffffff-a-b-c-d"),
            Err("Error at index 15 in: \"ffffffffffffffff\"".to_string())
        );
        // ...and the boundary just below `Long.MAX_VALUE` still parses.
        assert_eq!(
            Uuid::from_string("7fffffffffffffff-a-b-c-d"),
            Ok(uuid(0xffffffff_000a000b, 0x000c_00000000000d))
        );
    }

    #[test]
    fn canonical_uuid_round_trips_through_codec() {
        let entry = NameAndId::new(
            uuid(0x00112233_44556677, 0x8899_aabbccddeeff),
            "Notch".to_string(),
        );
        let codec = NameAndId::codec::<JsonOps>();
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &entry)
            .result()
            .cloned()
            .unwrap();
        // Java `NameAndId.CODEC` encode: `{"id": "...", "name": "Notch"}`.
        assert_eq!(
            encoded,
            serde_json::json!({
                "id": "00112233-4455-6677-8899-aabbccddeeff",
                "name": "Notch",
            })
        );
        let decoded: NameAndId = codec
            .parse(&JsonOps::INSTANCE, &encoded)
            .result()
            .cloned()
            .unwrap();
        assert_eq!(decoded, entry);
    }

    #[test]
    fn malformed_uuid_id_is_a_decode_error() {
        // A non-UUID `id` string makes the record decode error (Java
        // `UUID.fromString` throws). The lenient Players.sample field swallows
        // it on decode (empty sample), but the NameAndId codec itself errors.
        let json = serde_json::json!({"id": "not-a-uuid", "name": "X"});
        assert!(
            NameAndId::codec::<JsonOps>()
                .parse(&JsonOps::INSTANCE, &json)
                .result()
                .is_none()
        );
    }

    #[test]
    fn string_codec_error_matches_java_cause_suffix() {
        // Java `UUIDUtil.STRING_CODEC`:
        // `DataResult.error(() -> "Invalid UUID " + s + ": " + e.getMessage())`.
        // `UUID.fromString("nope")` throws `IllegalArgumentException("Invalid
        // UUID string: nope")` and the codec appends that cause verbatim.
        let result =
            uuid_string_codec::<JsonOps>().parse(&JsonOps::INSTANCE, &serde_json::json!("nope"));
        let err = result.error_ref().unwrap();
        assert_eq!(
            err.message(),
            "Invalid UUID nope: Invalid UUID string: nope"
        );
        // An overflowing group throws `NumberFormatException` from
        // `Long.parseLong` ("Error at index 15 in: ..."), appended the same way.
        let result = uuid_string_codec::<JsonOps>().parse(
            &JsonOps::INSTANCE,
            &serde_json::json!("8000000000000000-a-b-c-d"),
        );
        let err = result.error_ref().unwrap();
        assert_eq!(
            err.message(),
            "Invalid UUID 8000000000000000-a-b-c-d: Error at index 15 in: \"8000000000000000\""
        );
    }
}
