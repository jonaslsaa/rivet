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
//! at most 36 chars; braces/`urn:uuid:`/undashed are rejected) and rejects
//! malformed UUIDs with the Java `"Invalid UUID ..."` message.

use rivet_serialization::codec::{self, Codec};
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec;
use rivet_util::mth::Uuid;
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
/// (`UUID.fromString`, which throws `IllegalArgumentException` for a malformed
/// UUID — surfaced here as `DataResult.error` with Java's `"Invalid UUID ..."`
/// message prefix); on encode the canonical `UUID.toString()` form.
fn uuid_string_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<Uuid, Ops>> {
    codec::comap_flat_map(
        codec::string_codec::<Ops>(),
        Arc::new(|s: &String| match Uuid::from_string(s) {
            Some(uuid) => DataResult::success(uuid),
            None => DataResult::error(format!("Invalid UUID {s}")),
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
            Some(uuid(0x00112233_44556677, 0x8899_aabbccddeeff))
        );
        // Uppercase hex is accepted.
        assert_eq!(
            Uuid::from_string("FFFFFFFF-FFFF-FFFF-FFFF-FFFFFFFFFFFF"),
            Some(uuid(0xffffffff_ffffffff, 0xffff_ffffffffffff))
        );
        // Variable-width dashed groups pad with zeros (`fromString1`).
        assert_eq!(
            Uuid::from_string("1-2-3-4-5"),
            Some(uuid(0x00000001_00020003, 0x0004_000000000005))
        );
        // The all-zero UUID parses fine (no variant validation).
        assert_eq!(
            Uuid::from_string("00000000-0000-0000-0000-000000000000"),
            Some(uuid(0, 0))
        );
    }

    #[test]
    fn from_string_rejects_java_rejected_forms() {
        // Braces, `urn:uuid:`, the undashed 32-char form, and >36 chars are
        // all rejected by `UUID.fromString` ("UUID string too large" /
        // "Invalid UUID string").
        assert_eq!(
            Uuid::from_string("{00112233-4455-6677-8899-aabbccddeeff}"),
            None
        );
        assert_eq!(
            Uuid::from_string("urn:uuid:00112233-4455-6677-8899-aabbccddeeff"),
            None
        );
        assert_eq!(Uuid::from_string("00112233445566778899aabbccddeeff"), None);
        assert_eq!(
            Uuid::from_string("00112233-4455-6677-8899-aabbccddeefff"),
            None
        );
        // Empty groups and non-hex digits are NumberFormatException.
        assert_eq!(Uuid::from_string(""), None);
        assert_eq!(Uuid::from_string("00112233--6677-8899-aabbccddeeff"), None);
        assert_eq!(
            Uuid::from_string("00112233-4455-6677-8899-aabbccddeefg"),
            None
        );
        // A fifth dash means more than 4 groups.
        assert_eq!(
            Uuid::from_string("00112233-4455-6677-8899-aabbccddeeff-"),
            None
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
}
