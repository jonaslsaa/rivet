//! Port of `net.minecraft.network.protocol.status.ServerStatus` (issue #86).
//!
//! Java: `ServerStatus.java` in `working/Paper`. The status-response body: a
//! `Component` description, optional players/version/favicon, and the
//! `enforcesSecureChat` flag. `CODEC` is the exact `RecordCodecBuilder` group:
//!
//! ```text
//! description          lenientOptionalFieldOf("description", CommonComponents.EMPTY)
//! players              lenientOptionalFieldOf("players")            (Optional)
//! version              lenientOptionalFieldOf("version")            (Optional)
//! favicon              lenientOptionalFieldOf("favicon")            (Optional)
//! enforcesSecureChat   lenientOptionalFieldOf("enforcesSecureChat", false)
//! ```
//!
//! All five are lenient (a decode error or a missing key yields the default /
//! `None`), and the default-valued ones are OMITTED on encode when equal to
//! their default (Java's `optionalField(name, codec, lenient).xmap(o ->
//! o.orElse(default), a -> Objects.equals(a, default) ? Optional.empty() :
//! Optional.of(a))`). `ServerStatus` itself is never sent raw on the wire: the
//! packet wraps it via `lenientJson(32767).apply(fromCodec(OPS, CODEC))` (see
//! `clientbound_status_response_packet`).
//!
//! RivetTodo(#95): `Version.current()` needs `WorldVersion`/`SharedConstants`
//! (`mc.server`), so the value type is ported without the factory.

use crate::protocol::status::name_and_id::NameAndId;
use base64::Engine as _;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::{self, MapCodec};
use rivet_text::Component;
use rivet_text::component_serialization;
use std::sync::Arc;

/// `net.minecraft.network.protocol.status.ServerStatus`.
#[derive(Clone, Debug, PartialEq)]
pub struct ServerStatus {
    description: Component,
    players: Option<Players>,
    version: Option<Version>,
    favicon: Option<Favicon>,
    enforces_secure_chat: bool,
}

impl ServerStatus {
    /// `new ServerStatus(Component, Optional<Players>, Optional<Version>,
    /// Optional<Favicon>, boolean)`.
    pub fn new(
        description: Component,
        players: Option<Players>,
        version: Option<Version>,
        favicon: Option<Favicon>,
        enforces_secure_chat: bool,
    ) -> Self {
        ServerStatus {
            description,
            players,
            version,
            favicon,
            enforces_secure_chat,
        }
    }

    /// `ServerStatus.description()`.
    pub fn description(&self) -> &Component {
        &self.description
    }

    /// `ServerStatus.players()`.
    pub fn players(&self) -> Option<&Players> {
        self.players.as_ref()
    }

    /// `ServerStatus.version()`.
    pub fn version(&self) -> Option<&Version> {
        self.version.as_ref()
    }

    /// `ServerStatus.favicon()`.
    pub fn favicon(&self) -> Option<&Favicon> {
        self.favicon.as_ref()
    }

    /// `ServerStatus.enforcesSecureChat()`.
    pub fn enforces_secure_chat(&self) -> bool {
        self.enforces_secure_chat
    }

    /// `ServerStatus.CODEC` — the 5-field `RecordCodecBuilder` group (field
    /// order above).
    ///
    /// The `description` field is the recursive `ComponentSerialization.CODEC`
    /// (a permanent strong `Arc` cycle), so this is a registration-time
    /// constructor: build once per process and reuse (Java's `static final
    /// CODEC`). The status listener serves one response per ping, so
    /// [`crate::protocol::status::clientbound_status_response_packet::ClientboundStatusResponsePacket::stream_codec`]
    /// caches it behind a `static OnceLock` — do not call `codec()` per use.
    pub fn codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<ServerStatus, Ops>> {
        rivet_serialization::record_builder::create(move |instance| {
            let description = map_codec::for_getter(
                lenient_optional_field_of::<Component, Ops>(
                    "description",
                    component_serialization::codec(),
                    Component::empty(),
                ),
                Arc::new(|s: &ServerStatus| s.description.clone()),
            );
            let players = map_codec::for_getter(
                codec::optional_field("players".to_string(), Players::codec::<Ops>(), true),
                Arc::new(|s: &ServerStatus| s.players.clone()),
            );
            let version = map_codec::for_getter(
                codec::optional_field("version".to_string(), Version::codec::<Ops>(), true),
                Arc::new(|s: &ServerStatus| s.version.clone()),
            );
            let favicon = map_codec::for_getter(
                codec::optional_field("favicon".to_string(), Favicon::codec::<Ops>(), true),
                Arc::new(|s: &ServerStatus| s.favicon.clone()),
            );
            let enforces_secure_chat = map_codec::for_getter(
                lenient_optional_field_of::<bool, Ops>(
                    "enforcesSecureChat",
                    codec::bool_codec(),
                    false,
                ),
                Arc::new(|s: &ServerStatus| s.enforces_secure_chat),
            );
            instance
                .group(description)
                .and(players)
                .and(version)
                .and(favicon)
                .and(enforces_secure_chat)
                .apply(
                    instance,
                    Arc::new(
                        |description: Component,
                         players: Option<Players>,
                         version: Option<Version>,
                         favicon: Option<Favicon>,
                         enforces_secure_chat: bool| {
                            ServerStatus::new(
                                description,
                                players,
                                version,
                                favicon,
                                enforces_secure_chat,
                            )
                        },
                    ),
                )
        })
    }
}

/// `ServerStatus.Players` — `(int max, int online, List<NameAndId> sample)`.
#[derive(Clone, Debug, PartialEq)]
pub struct Players {
    max: i32,
    online: i32,
    sample: Vec<NameAndId>,
}

impl Players {
    /// `new Players(int max, int online, List<NameAndId> sample)`.
    pub fn new(max: i32, online: i32, sample: Vec<NameAndId>) -> Self {
        Players {
            max,
            online,
            sample,
        }
    }

    /// `ServerStatus.Players.max()`.
    pub fn max(&self) -> i32 {
        self.max
    }

    /// `ServerStatus.Players.online()`.
    pub fn online(&self) -> i32 {
        self.online
    }

    /// `ServerStatus.Players.sample()`.
    pub fn sample(&self) -> &[NameAndId] {
        &self.sample
    }

    /// `ServerStatus.Players.CODEC` — `record { max: INT, online: INT,
    /// sample: NameAndId.CODEC.listOf().lenientOptionalFieldOf("sample",
    /// List.of()) }`.
    pub fn codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<Players, Ops>> {
        rivet_serialization::record_builder::create(move |instance| {
            let max = map_codec::for_getter(
                codec::field_of(codec::int_codec::<Ops>(), "max".to_string()),
                Arc::new(|p: &Players| p.max),
            );
            let online = map_codec::for_getter(
                codec::field_of(codec::int_codec::<Ops>(), "online".to_string()),
                Arc::new(|p: &Players| p.online),
            );
            let sample = map_codec::for_getter(
                lenient_optional_field_of::<Vec<NameAndId>, Ops>(
                    "sample",
                    codec::list(NameAndId::codec::<Ops>()),
                    Vec::new(),
                ),
                Arc::new(|p: &Players| p.sample.clone()),
            );
            instance.group(max).and(online).and(sample).apply(
                instance,
                Arc::new(|max: i32, online: i32, sample: Vec<NameAndId>| {
                    Players::new(max, online, sample)
                }),
            )
        })
    }
}

/// `ServerStatus.Version` — `(String name, int protocol)`.
#[derive(Clone, Debug, PartialEq)]
pub struct Version {
    name: String,
    protocol: i32,
}

impl Version {
    /// `new Version(String name, int protocol)`.
    pub fn new(name: String, protocol: i32) -> Self {
        Version { name, protocol }
    }

    /// `ServerStatus.Version.name()`.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// `ServerStatus.Version.protocol()`.
    pub fn protocol(&self) -> i32 {
        self.protocol
    }

    /// `ServerStatus.Version.CODEC` — `record { name: STRING, protocol: INT }`.
    pub fn codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<Version, Ops>> {
        rivet_serialization::record_builder::create(move |instance| {
            let name = map_codec::for_getter(
                codec::field_of(codec::string_codec::<Ops>(), "name".to_string()),
                Arc::new(|v: &Version| v.name.clone()),
            );
            let protocol = map_codec::for_getter(
                codec::field_of(codec::int_codec::<Ops>(), "protocol".to_string()),
                Arc::new(|v: &Version| v.protocol),
            );
            instance.group(name).and(protocol).apply(
                instance,
                Arc::new(|name: String, protocol: i32| Version::new(name, protocol)),
            )
        })
    }
}

/// `ServerStatus.Favicon` — `(byte[] iconBytes)`, a base64 PNG carried with
/// the `data:image/png;base64,` prefix.
#[derive(Clone, Debug, PartialEq)]
pub struct Favicon {
    icon_bytes: Vec<u8>,
}

/// `ServerStatus.Favicon.PREFIX`.
pub const FAVICON_PREFIX: &str = "data:image/png;base64,";

impl Favicon {
    /// `new Favicon(byte[] iconBytes)`.
    pub fn new(icon_bytes: Vec<u8>) -> Self {
        Favicon { icon_bytes }
    }

    /// `ServerStatus.Favicon.iconBytes()`.
    pub fn icon_bytes(&self) -> &[u8] {
        &self.icon_bytes
    }

    /// `ServerStatus.Favicon.CODEC` — `Codec.STRING.comapFlatMap(parse,
    /// render)`. Decode: the string must start with the PNG prefix (else
    /// `"Unknown format"`), the rest is base64-decoded after stripping `\n`
    /// (else `"Malformed base64 server icon"`). Encode: prefix + base64 of the
    /// raw bytes.
    pub fn codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<Favicon, Ops>> {
        codec::comap_flat_map(
            codec::string_codec::<Ops>(),
            Arc::new(
                |s: &String| -> rivet_serialization::data_result::DataResult<Favicon> {
                    if !s.starts_with(FAVICON_PREFIX) {
                        return rivet_serialization::data_result::DataResult::error(
                            "Unknown format",
                        );
                    }
                    let base64 = s[FAVICON_PREFIX.len()..].replace('\n', "");
                    match base64::engine::general_purpose::STANDARD.decode(&base64) {
                        Ok(icon_bytes) => rivet_serialization::data_result::DataResult::success(
                            Favicon::new(icon_bytes),
                        ),
                        Err(_) => rivet_serialization::data_result::DataResult::error(
                            "Malformed base64 server icon",
                        ),
                    }
                },
            ),
            Arc::new(|f: &Favicon| {
                format!(
                    "{FAVICON_PREFIX}{}",
                    base64::engine::general_purpose::STANDARD.encode(&f.icon_bytes)
                )
            }),
        )
    }
}

/// `Codec.lenientOptionalFieldOf(String, F default)` — the with-default form
/// of a lenient optional field (Java `optionalField(name, codec, lenient)
/// .xmap(o -> o.orElse(default), a -> Objects.equals(a, default) ?
/// Optional.empty() : Optional.of(a))`): the field value defaults on decode
/// and is OMITTED on encode when equal to `default`.
fn lenient_optional_field_of<F, Ops>(
    name: &str,
    element_codec: Arc<dyn Codec<F, Ops>>,
    default: F,
) -> Arc<dyn MapCodec<F, Ops>>
where
    F: Clone + PartialEq + Send + Sync + 'static,
    Ops: DynamicOps + 'static,
{
    let inner = codec::optional_field(name.to_string(), element_codec, true);
    let default_for_decode = default.clone();
    let default_for_encode = default;
    map_codec::xmap(
        inner,
        Arc::new(move |o: &Option<F>| o.clone().unwrap_or_else(|| default_for_decode.clone())),
        Arc::new(move |a: &F| {
            if *a == default_for_encode {
                None
            } else {
                Some(a.clone())
            }
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_serialization::codec::Codec;
    use rivet_serialization::json_ops::JsonOps;

    fn status(description: Component) -> ServerStatus {
        ServerStatus::new(description, None, None, None, false)
    }

    fn version() -> Version {
        Version::new("1.21.4".to_string(), 769)
    }

    /// Encode `value` to a `serde_json::Value` (the JSON ops output).
    fn encode_json<A: 'static>(codec: Arc<dyn Codec<A, JsonOps>>, value: &A) -> serde_json::Value {
        codec
            .encode_start(&JsonOps::INSTANCE, value)
            .result()
            .cloned()
            .unwrap()
    }

    /// Decode `json` (a `serde_json::Value`) with the codec.
    fn decode_json<A: 'static + Clone>(
        codec: Arc<dyn Codec<A, JsonOps>>,
        json: serde_json::Value,
    ) -> A {
        codec
            .parse(&JsonOps::INSTANCE, &json)
            .result()
            .cloned()
            .unwrap()
    }

    #[test]
    fn default_only_status_round_trips() {
        // Java: `ServerStatus.CODEC.encodeStart` of a default status yields
        // `{}` — every field is `lenientOptionalFieldOf` with a default
        // (`CommonComponents.EMPTY`, `Optional.empty()`, `false`), so all five
        // are equal to their default and OMITTED.
        let codec = ServerStatus::codec::<JsonOps>();
        let encoded = encode_json(codec.clone(), &status(Component::empty()));
        let expected = serde_json::json!({});
        assert_eq!(encoded, expected);
        assert_eq!(decode_json(codec, encoded), status(Component::empty()));
    }

    #[test]
    fn full_status_round_trips_with_field_order() {
        let players = Players::new(
            20,
            3,
            vec![NameAndId::new(
                rivet_util::uuid::Uuid {
                    most: 0x00112233_44556677,
                    least: 0x8899aabb_ccddeeffu64 as i64,
                },
                "Notch".to_string(),
            )],
        );
        let status = ServerStatus::new(
            Component::literal("Hello"),
            Some(players),
            Some(version()),
            Some(Favicon::new(vec![0x89, 0x50, 0x4e, 0x47])),
            true,
        );
        let codec = ServerStatus::codec::<JsonOps>();
        let encoded = encode_json(codec.clone(), &status);
        // Field order: description, players, version, favicon, enforcesSecureChat.
        assert_eq!(
            encoded.as_object().unwrap().keys().collect::<Vec<_>>(),
            vec![
                "description",
                "players",
                "version",
                "favicon",
                "enforcesSecureChat"
            ]
        );
        assert_eq!(decode_json(codec, encoded), status);
    }

    #[test]
    fn missing_optional_fields_default() {
        // `players`/`version`/`favicon` missing -> None; `description` and
        // `enforcesSecureChat` -> defaults.
        let json = serde_json::json!({"description": "x"});
        let decoded = decode_json(ServerStatus::codec::<JsonOps>(), json);
        assert_eq!(
            decoded,
            ServerStatus::new(Component::literal("x"), None, None, None, false)
        );
    }

    #[test]
    fn malformed_favicon_prefix_errors() {
        // Java `Favicon.CODEC` decode of a non-prefixed string is
        // `DataResult.error("Unknown format")`.
        let result = Favicon::codec::<JsonOps>()
            .parse(&JsonOps::INSTANCE, &serde_json::json!("not a favicon"));
        let err = result.error_ref().unwrap();
        assert_eq!(err.message(), "Unknown format");
    }

    #[test]
    fn malformed_base64_errors() {
        // Java: `Base64.getDecoder().decode` throws `IllegalArgumentException`
        // -> `DataResult.error("Malformed base64 server icon")`.
        let result = Favicon::codec::<JsonOps>().parse(
            &JsonOps::INSTANCE,
            &serde_json::json!("data:image/png;base64,!!!!"),
        );
        let err = result.error_ref().unwrap();
        assert_eq!(err.message(), "Malformed base64 server icon");
    }

    #[test]
    fn favicon_round_trips_stripping_newlines() {
        // Encode: prefix + base64. Decode: strips `\n` before base64-decode.
        let favicon = Favicon::new(vec![1, 2, 3, 4]);
        let codec = Favicon::codec::<JsonOps>();
        let encoded = encode_json(codec.clone(), &favicon);
        assert_eq!(encoded, serde_json::json!("data:image/png;base64,AQIDBA=="));
        assert_eq!(decode_json(codec, encoded), favicon);
    }

    #[test]
    fn sample_omitted_when_empty_but_present_when_nonempty() {
        // `sample` is `lenientOptionalFieldOf("sample", List.of())` — omitted
        // on encode when empty, present when it has entries.
        let empty = Players::new(10, 0, Vec::new());
        let codec = Players::codec::<JsonOps>();
        assert_eq!(
            encode_json(codec.clone(), &empty),
            serde_json::json!({"max": 10, "online": 0})
        );
        let full = Players::new(
            10,
            1,
            vec![NameAndId::new(
                rivet_util::uuid::Uuid { most: 1, least: 2 },
                "A".to_string(),
            )],
        );
        let encoded = encode_json(codec.clone(), &full);
        assert!(encoded.as_object().unwrap().contains_key("sample"));
        assert_eq!(decode_json(codec, encoded), full);
    }
}
