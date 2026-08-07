//! Port of `net.minecraft.network.protocol.game.ClientboundPlayerInfoUpdatePacket`
//! (issue #87) — `player_info_update` (play clientbound id 70).
//!
//! Java source: `.../network/protocol/game/ClientboundPlayerInfoUpdatePacket.java`.
//! Wire body: a fixed bit-set of the eight `Action` flags (`writeEnumSet`; a
//! fixed `Mth.positiveCeilDiv(8, 8) = 1` byte, least-significant bit is the
//! `ADD_PLAYER` ordinal), then a varint count of `Entry`s. Each entry is a UUID
//! followed by one field per action, in the actions' ordinal order:
//!
//! | action | per-entry field | wire form |
//! |---|---|---|
//! | `ADD_PLAYER` | `profile` | `PLAYER_NAME` (utf8 ≤16) + `GAME_PROFILE_PROPERTIES` (varint-counted `PropertyMap`) |
//! | `INITIALIZE_CHAT` | `chatSession` | nullable `RemoteChatSession.Data` |
//! | `UPDATE_GAME_MODE` | `gameMode` | varint `GameType.getId` |
//! | `UPDATE_LISTED` | `listed` | boolean |
//! | `UPDATE_LATENCY` | `latency` | varint |
//! | `UPDATE_DISPLAY_NAME` | `displayName` | nullable trusted `Component` |
//! | `UPDATE_LIST_ORDER` | `listOrder` | varint |
//! | `UPDATE_HAT` | `showHat` | boolean |
//!
//! The captured golden body (`join_clientbound_player_info_update.hex`, 37
//! bytes) has all eight actions set and one `RivetProbe` entry carrying the
//! offline `GameProfile` (name `RivetProbe`, no properties), `chatSession =
//! null`, `gameMode 0`, `listed 1`, `latency 0`, no display name, `listOrder 0`,
//! `showHat 1`.
//!
//! `EnumSet` ports as a sorted [`Vec<Action>`] (the `Action` derives
//! `Ord`, so `Vec::sort` reproduces Java's ordinal iteration for both the
//! writer's per-entry loop and the bit-set construction). Display-name
//! encoding carries the raw trusted `Component` wire form — an NBT tag via
//! [`trusted_tag`] — the `NbtOps` parse into a `Component` lands with the
//! text unit (RivetTodo(#206)), so a `None` display name stays absent.
//! `RemoteChatSession.Data`/`ProfilePublicKey.Data` are the deferred chat
//! unit's wire value; only their plain-`FriendlyByteBuf` shape ports here.

use crate::codec::byte_buf_codecs::{player_name, trusted_tag};
use crate::codec::{CodecError, StreamCodec, StreamDecoder, StreamEncoder, codec};
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::protocol::game::packet_types::clientbound_player_info_update;
use crate::protocol::packet::Packet;
use crate::protocol::packet_type::PacketType;
use rivet_registry::core::GameType;
use rivet_registry::core::{GameProfile, PropertyMap};

/// `ClientboundPlayerInfoUpdatePacket.Action` — the eight actions, in Java
/// declaration order. The enum's ordinal is the bit index in the wire bit-set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Action {
    /// `ADD_PLAYER`.
    AddPlayer,
    /// `INITIALIZE_CHAT`.
    InitializeChat,
    /// `UPDATE_GAME_MODE`.
    UpdateGameMode,
    /// `UPDATE_LISTED`.
    UpdateListed,
    /// `UPDATE_LATENCY`.
    UpdateLatency,
    /// `UPDATE_DISPLAY_NAME`.
    UpdateDisplayName,
    /// `UPDATE_LIST_ORDER`.
    UpdateListOrder,
    /// `UPDATE_HAT`.
    UpdateHat,
}

/// The eight `Action` values in ordinal order.
pub const ACTIONS: [Action; 8] = [
    Action::AddPlayer,
    Action::InitializeChat,
    Action::UpdateGameMode,
    Action::UpdateListed,
    Action::UpdateLatency,
    Action::UpdateDisplayName,
    Action::UpdateListOrder,
    Action::UpdateHat,
];

impl Action {
    /// The Java `ordinal()` — the bit index in the wire bit-set.
    pub fn ordinal(self) -> usize {
        self as usize
    }
}

/// `ClientboundPlayerInfoUpdatePacket.Entry` — the per-player payload. `profile`
/// is `null` when `ADD_PLAYER` is absent (a protocol 776 offline join); Java
/// models that as a nullable `GameProfile`, and the decode-ctor only builds it
/// when the action is present. `display_name`/`chat_session` are the nullable
/// component / chat-session values.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// `profileId` — the player UUID, always present.
    profile_id: rivet_util::mth::Uuid,
    /// `profile` — Java `@Nullable GameProfile`.
    profile: Option<GameProfile>,
    /// `listed`.
    listed: bool,
    /// `latency`.
    latency: i32,
    /// `gameMode`.
    game_mode: GameType,
    /// `displayName` — Java `@Nullable Component` (wire: nullable NBT `Tag`).
    ///
    /// RivetTodo(#206): Java's `ComponentSerialization.TRUSTED_STREAM_CODEC`
    /// reads an NBT tag then parses it into a `Component` via `NbtOps`. `NbtOps`
    /// is not yet ported (epic #12), so the raw tag is carried as the value; the
    /// `Component`-typed surface is added with the text unit. The wire bytes are
    /// identical (the tag is the payload).
    display_name: Option<rivet_nbt::tag::Tag>,
    /// `showHat`.
    show_hat: bool,
    /// `listOrder`.
    list_order: i32,
    /// `chatSession` — Java `@Nullable RemoteChatSession.Data`.
    chat_session: Option<RemoteChatSessionData>,
}

impl Entry {
    /// The record's canonical constructor.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        profile_id: rivet_util::mth::Uuid,
        profile: Option<GameProfile>,
        listed: bool,
        latency: i32,
        game_mode: GameType,
        display_name: Option<rivet_nbt::tag::Tag>,
        show_hat: bool,
        list_order: i32,
        chat_session: Option<RemoteChatSessionData>,
    ) -> Self {
        Entry {
            profile_id,
            profile,
            listed,
            latency,
            game_mode,
            display_name,
            show_hat,
            list_order,
            chat_session,
        }
    }

    /// `Entry.profileId()`.
    pub fn profile_id(&self) -> rivet_util::mth::Uuid {
        self.profile_id
    }

    /// `Entry.profile()` — the `@Nullable GameProfile`.
    pub fn profile(&self) -> Option<&GameProfile> {
        self.profile.as_ref()
    }

    /// `Entry.listed()`.
    pub fn listed(&self) -> bool {
        self.listed
    }

    /// `Entry.latency()`.
    pub fn latency(&self) -> i32 {
        self.latency
    }

    /// `Entry.gameMode()`.
    pub fn game_mode(&self) -> GameType {
        self.game_mode
    }

    /// `Entry.displayName()`.
    pub fn display_name(&self) -> Option<&rivet_nbt::tag::Tag> {
        self.display_name.as_ref()
    }

    /// `Entry.showHat()`.
    pub fn show_hat(&self) -> bool {
        self.show_hat
    }

    /// `Entry.listOrder()`.
    pub fn list_order(&self) -> i32 {
        self.list_order
    }

    /// `Entry.chatSession()`.
    pub fn chat_session(&self) -> Option<&RemoteChatSessionData> {
        self.chat_session.as_ref()
    }
}

/// `RemoteChatSession.Data` — `(UUID sessionId, ProfilePublicKey.Data
/// profilePublicKey)`, the deferred chat unit's wire value. Only the wire shape
/// the `INITIALIZE_CHAT` action reads/writes ports here (OWNERSHIP.md: the chat
/// unit owns the full record in `rivet-world`). `ProfilePublicKey.Data` is an
/// epoch-milli `Instant`, a DER-encoded public key, and a `keySignature` byte
/// array capped at 4096 — the plain `FriendlyByteBuf` primitive surface.
#[derive(Debug, Clone, PartialEq)]
pub struct RemoteChatSessionData {
    /// `sessionId`.
    session_id: rivet_util::mth::Uuid,
    /// `profilePublicKey`.
    profile_public_key: ProfilePublicKeyData,
}

impl RemoteChatSessionData {
    /// `RemoteChatSession.Data.read(FriendlyByteBuf)`.
    fn read(input: &mut FriendlyByteBuf) -> Self {
        RemoteChatSessionData {
            session_id: input.read_uuid(),
            profile_public_key: ProfilePublicKeyData::read(input),
        }
    }

    /// `RemoteChatSession.Data.write(FriendlyByteBuf, Data)`.
    fn write(&self, output: &mut FriendlyByteBuf) {
        output.write_uuid(self.session_id);
        self.profile_public_key.write(output);
    }
}

/// `ProfilePublicKey.Data` — `(Instant expiresAt, PublicKey key, byte[]
/// keySignature)`, the deferred chat unit's wire value. The `Instant` is a
/// big-endian `long` epoch-milli; the public key is a big-endian `int` length +
/// DER bytes; the signature is a `readByteArray(4096)`.
#[derive(Debug, Clone, PartialEq)]
pub struct ProfilePublicKeyData {
    /// `expiresAt` (epoch-milli, big-endian long).
    expires_at: i64,
    /// `key` (DER-encoded bytes).
    key: Vec<u8>,
    /// `keySignature` — `readByteArray(4096)`.
    key_signature: Vec<u8>,
}

impl ProfilePublicKeyData {
    /// `ProfilePublicKey.Data(FriendlyByteBuf)`.
    fn read(input: &mut FriendlyByteBuf) -> Self {
        ProfilePublicKeyData {
            expires_at: input.read_long(),
            key: read_public_key(input),
            key_signature: input.read_byte_array_max(4096),
        }
    }

    /// `ProfilePublicKey.Data.write(FriendlyByteBuf)`.
    fn write(&self, output: &mut FriendlyByteBuf) {
        output.write_long(self.expires_at);
        write_public_key(output, &self.key);
        output.write_byte_array(&self.key_signature);
    }
}

/// `FriendlyByteBuf.readPublicKey()` — a varint length-prefixed `PublicKey`
/// (the DER `X509EncodedKeySpec` bytes), bounded at
/// `MAX_PUBLIC_KEY_LENGTH = 512` like Java's `readByteArray(512)`.
fn read_public_key(input: &mut FriendlyByteBuf) -> Vec<u8> {
    input.read_byte_array_max(512)
}

/// `FriendlyByteBuf.writePublicKey(PublicKey)` — the key length as a varint,
/// then the DER bytes.
fn write_public_key(output: &mut FriendlyByteBuf, key: &[u8]) {
    output.write_var_int(key.len() as i32);
    output.write_bytes(key);
}

/// `ClientboundPlayerInfoUpdatePacket` — the (fixed) set of actions + list of
/// `Entry`s.
#[derive(Debug, Clone, PartialEq)]
pub struct ClientboundPlayerInfoUpdatePacket {
    /// `actions` — a sorted `Vec` standing in for Java's `EnumSet`.
    actions: Vec<Action>,
    /// `entries`.
    entries: Vec<Entry>,
}

impl ClientboundPlayerInfoUpdatePacket {
    /// The record's canonical constructor (Paper's `(EnumSet, List<Entry>)`).
    pub fn new(actions: Vec<Action>, entries: Vec<Entry>) -> Self {
        let mut actions = actions;
        actions.sort();
        actions.dedup();
        ClientboundPlayerInfoUpdatePacket { actions, entries }
    }

    /// `ClientboundPlayerInfoUpdatePacket.actions()`.
    pub fn actions(&self) -> &[Action] {
        &self.actions
    }

    /// `ClientboundPlayerInfoUpdatePacket.entries()`.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// `STREAM_CODEC` — `Packet.codec(write, new(RegistryFriendlyByteBuf))`.
    pub fn stream_codec() -> StreamCodec<FriendlyByteBuf, ClientboundPlayerInfoUpdatePacket> {
        codec(
            |packet: &ClientboundPlayerInfoUpdatePacket, output: &mut FriendlyByteBuf| {
                let bits = packet_actions_bits(&packet.actions);
                write_enum_set(output, bits);
                output.write_var_int(packet.entries.len() as i32);
                for entry in &packet.entries {
                    entry.write(output, &packet.actions)?;
                }
                Ok(())
            },
            |input: &mut FriendlyByteBuf| {
                let bits = read_enum_set(input);
                let mut actions: Vec<Action> = ACTIONS
                    .iter()
                    .copied()
                    .filter(|a| (bits >> a.ordinal()) & 1 == 1)
                    .collect();
                actions.sort();
                let count = input.read_var_int();
                let mut entries = Vec::with_capacity(count as usize);
                for _ in 0..count {
                    entries.push(Entry::read(input, &actions)?);
                }
                Ok(ClientboundPlayerInfoUpdatePacket { actions, entries })
            },
        )
    }
}

/// `FriendlyByteBuf.writeEnumSet(set, Action.class)` — `writeFixedBitSet(mask,
/// 8)` = one byte (the `Mth.positiveCeilDiv(8, 8)` fixed length), the
/// least-significant bit being the `ADD_PLAYER` ordinal.
fn write_enum_set(output: &mut FriendlyByteBuf, bits: u8) {
    output.write_byte(bits as i8);
}

/// `FriendlyByteBuf.readEnumSet(Action.class)` — `readFixedBitSet(8)` = one
/// byte.
fn read_enum_set(input: &mut FriendlyByteBuf) -> u8 {
    input.read_unsigned_byte()
}

/// The `EnumSet`'s 8-bit mask from a sorted action `Vec`.
fn packet_actions_bits(actions: &[Action]) -> u8 {
    let mut bits = 0u8;
    for action in actions {
        bits |= 1u8 << action.ordinal();
    }
    bits
}

impl Entry {
    /// The per-entry read — `input.readList(buf -> { builder = new EntryBuilder(
    /// buf.readUUID()); for (action : actions) action.reader.read(builder, buf);
    /// return builder.build(); })`. The Rust port threads the actions set and
    /// reads each action's field in ordinal order.
    fn read(input: &mut FriendlyByteBuf, actions: &[Action]) -> Result<Entry, CodecError> {
        let profile_id = input.read_uuid();
        let mut profile = None;
        let mut listed = false;
        let mut latency = 0;
        let mut game_mode = GameType::DEFAULT_MODE;
        let mut display_name = None;
        let mut show_hat = false;
        let mut list_order = 0;
        let mut chat_session = None;

        for action in ACTIONS {
            if !actions.contains(&action) {
                continue;
            }
            match action {
                Action::AddPlayer => {
                    let name = player_name().decode(input)?;
                    let properties: PropertyMap =
                        crate::codec::byte_buf_codecs::game_profile_properties().decode(input)?;
                    profile = Some(GameProfile::new(profile_id, name, properties));
                }
                Action::InitializeChat => {
                    chat_session = read_nullable(input, RemoteChatSessionData::read);
                }
                Action::UpdateGameMode => {
                    game_mode = GameType::by_id(input.read_var_int());
                }
                Action::UpdateListed => {
                    listed = input.read_boolean();
                }
                Action::UpdateLatency => {
                    latency = input.read_var_int();
                }
                Action::UpdateDisplayName => {
                    display_name = read_nullable_result(input, |buf| trusted_tag().decode(buf))?;
                }
                Action::UpdateListOrder => {
                    list_order = input.read_var_int();
                }
                Action::UpdateHat => {
                    show_hat = input.read_boolean();
                }
            }
        }

        Ok(Entry {
            profile_id,
            profile,
            listed,
            latency,
            game_mode,
            display_name,
            show_hat,
            list_order,
            chat_session,
        })
    }

    /// The per-entry write — `writeCollection(entries, (buf, entry) -> {
    /// buf.writeUUID(entry.profileId()); for (action : actions) action.writer.
    /// write(buf, entry); })`.
    fn write(&self, output: &mut FriendlyByteBuf, actions: &[Action]) -> Result<(), CodecError> {
        output.write_uuid(self.profile_id);
        for action in ACTIONS {
            if !actions.contains(&action) {
                continue;
            }
            match action {
                Action::AddPlayer => {
                    // `Objects.requireNonNull(entry.profile())` — a packet with
                    // ADD_PLAYER but no profile panics, like Java's NPE.
                    let profile = self.profile.as_ref().ok_or_else(|| {
                        CodecError::new(
                            "Cannot invoke \"GameProfile.name()\" because \"profile\" is null",
                        )
                    })?;
                    player_name().encode(output, &profile.name().to_string())?;
                    crate::codec::byte_buf_codecs::game_profile_properties()
                        .encode(output, profile.properties())?;
                }
                Action::InitializeChat => {
                    let mut chat = self.chat_session.as_ref();
                    // Paper: `if (chatSession != null && chatSession.
                    // profilePublicKey().hasExpired()) chatSession = null;`
                    if let Some(data) = chat
                        && data.profile_public_key.expires_at < now_epoch_milli()
                    {
                        chat = None;
                    }
                    write_nullable(output, chat, |buf, data| data.write(buf));
                }
                Action::UpdateGameMode => {
                    output.write_var_int(self.game_mode.get_id());
                }
                Action::UpdateListed => {
                    output.write_boolean(self.listed);
                }
                Action::UpdateLatency => {
                    output.write_var_int(self.latency);
                }
                Action::UpdateDisplayName => {
                    write_nullable_result(output, self.display_name.as_ref(), |buf, value| {
                        // RivetTodo(#206): encode the raw tag; the Component
                        // codec path lands with the text unit (epic #12).
                        trusted_tag().encode(buf, value)
                    })?;
                }
                Action::UpdateListOrder => {
                    output.write_var_int(self.list_order);
                }
                Action::UpdateHat => {
                    output.write_boolean(self.show_hat);
                }
            }
        }
        Ok(())
    }
}

/// `FriendlyByteBuf.readNullable(buf, reader)` — a boolean prefix then the
/// value, `None` when the flag is false.
fn read_nullable<T>(
    input: &mut FriendlyByteBuf,
    reader: impl FnOnce(&mut FriendlyByteBuf) -> T,
) -> Option<T> {
    if input.read_boolean() {
        Some(reader(input))
    } else {
        None
    }
}

/// `FriendlyByteBuf.readNullable(buf, reader)` where the reader returns
/// `Result` — the `ComponentSerialization.TRUSTED_STREAM_CODEC` decode form
/// (`readNullable` threads the codec's `DecoderException`).
fn read_nullable_result<T>(
    input: &mut FriendlyByteBuf,
    reader: impl FnOnce(&mut FriendlyByteBuf) -> Result<T, CodecError>,
) -> Result<Option<T>, CodecError> {
    if input.read_boolean() {
        reader(input).map(Some)
    } else {
        Ok(None)
    }
}

/// `FriendlyByteBuf.writeNullable(buf, value, writer)` — a boolean prefix then
/// the value.
fn write_nullable<T>(
    output: &mut FriendlyByteBuf,
    value: Option<&T>,
    writer: impl FnOnce(&mut FriendlyByteBuf, &T),
) {
    match value {
        Some(value) => {
            output.write_boolean(true);
            writer(output, value);
        }
        None => {
            output.write_boolean(false);
        }
    }
}

/// `FriendlyByteBuf.writeNullable(buf, value, writer)` where the writer returns
/// `Result` — the `TRUSTED_STREAM_CODEC` encode form (the codec's
/// `EncoderException` propagates).
fn write_nullable_result<T>(
    output: &mut FriendlyByteBuf,
    value: Option<&T>,
    writer: impl FnOnce(&mut FriendlyByteBuf, &T) -> Result<(), CodecError>,
) -> Result<(), CodecError> {
    match value {
        Some(value) => {
            output.write_boolean(true);
            writer(output, value)
        }
        None => {
            output.write_boolean(false);
            Ok(())
        }
    }
}

/// The current epoch-milli (Paper's `ProfilePublicKey.Data.hasExpired()` uses
/// `Instant.now()`).
fn now_epoch_milli() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

impl Packet for ClientboundPlayerInfoUpdatePacket {
    fn packet_type(&self) -> PacketType {
        clientbound_player_info_update()
    }
}
