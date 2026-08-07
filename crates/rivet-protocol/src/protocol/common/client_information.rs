//! Port of `net.minecraft.server.level.ClientInformation` (issue #197).
//!
//! Java: `ClientInformation.java` in `working/Paper`. The client's
//! configuration value, decoded/encoded by `ServerboundClientInformationPacket`
//! (configuration serverbound id 0, play serverbound id 14). Field order and
//! wire encoding mirror the Java record's `(FriendlyByteBuf)` constructor /
//! `write(FriendlyByteBuf)` exactly:
//!
//!   1. `language`     — `readUtf(16)` / `writeUtf(language)`
//!   2. `viewDistance` — `readByte` / `writeByte` (signed byte; a negative
//!      value decodes as a valid value — configuration stores it, the play
//!      listener rejects it, issue #96)
//!   3. `chatVisibility`      — `readEnum` / `writeEnum` (ordinal varint)
//!   4. `chatColors`          — `readBoolean` / `writeBoolean`
//!   5. `modelCustomisation`  — `readUnsignedByte` / `writeByte` (0..=255 mask)
//!   6. `mainHand`            — `readEnum` / `writeEnum` (ordinal varint)
//!   7. `textFilteringEnabled`— `readBoolean` / `writeBoolean`
//!   8. `allowsListing`       — `readBoolean` / `writeBoolean`
//!   9. `particleStatus`      — `readEnum` / `writeEnum` (ordinal varint)
//!
//! The language boundary is asymmetric exactly as Java's: encode uses
//! `writeUtf(CharSequence)` (max `MAX_STRING_LENGTH`), decode uses `readUtf(16)`
//! (max [`MAX_LANGUAGE_LENGTH`] UTF-16 units), so the encode side accepts a
//! language longer than 16 code units that decode will later reject.
//!
//! Placement note: `ClientInformation` is a `net.minecraft.server.level` value
//! type, but it lives in `rivet-protocol` (not `rivet-server`, its
//! package-mirror home) because the packet body that needs it is here and
//! `rivet-server` is downstream of `rivet-protocol`.

use crate::codec::byte_buf_codecs;
use crate::codec::{CodecError, StreamCodec, StreamDecoder, of};
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::protocol::common::chat_visiblity::ChatVisiblity;
use crate::protocol::common::humanoid_arm::HumanoidArm;
use crate::protocol::common::particle_status::ParticleStatus;

/// `ClientInformation.MAX_LANGUAGE_LENGTH`.
pub const MAX_LANGUAGE_LENGTH: i32 = 16;

/// `net.minecraft.server.level.ClientInformation`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClientInformation {
    language: String,
    view_distance: i8,
    chat_visibility: ChatVisiblity,
    chat_colors: bool,
    model_customisation: u8,
    main_hand: HumanoidArm,
    text_filtering_enabled: bool,
    allows_listing: bool,
    particle_status: ParticleStatus,
}

impl ClientInformation {
    /// `new ClientInformation(String language, int viewDistance,
    /// ChatVisiblity chatVisibility, boolean chatColors, int
    /// modelCustomisation, HumanoidArm mainHand, boolean textFilteringEnabled,
    /// boolean allowsListing, ParticleStatus particleStatus)`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        language: String,
        view_distance: i8,
        chat_visibility: ChatVisiblity,
        chat_colors: bool,
        model_customisation: u8,
        main_hand: HumanoidArm,
        text_filtering_enabled: bool,
        allows_listing: bool,
        particle_status: ParticleStatus,
    ) -> Self {
        ClientInformation {
            language,
            view_distance,
            chat_visibility,
            chat_colors,
            model_customisation,
            main_hand,
            text_filtering_enabled,
            allows_listing,
            particle_status,
        }
    }

    /// `ClientInformation.language()`.
    pub fn language(&self) -> &str {
        &self.language
    }

    /// `ClientInformation.viewDistance()`.
    pub fn view_distance(&self) -> i8 {
        self.view_distance
    }

    /// `ClientInformation.chatVisibility()`.
    pub fn chat_visibility(&self) -> ChatVisiblity {
        self.chat_visibility
    }

    /// `ClientInformation.chatColors()`.
    pub fn chat_colors(&self) -> bool {
        self.chat_colors
    }

    /// `ClientInformation.modelCustomisation()`.
    pub fn model_customisation(&self) -> u8 {
        self.model_customisation
    }

    /// `ClientInformation.mainHand()`.
    pub fn main_hand(&self) -> HumanoidArm {
        self.main_hand
    }

    /// `ClientInformation.textFilteringEnabled()`.
    pub fn text_filtering_enabled(&self) -> bool {
        self.text_filtering_enabled
    }

    /// `ClientInformation.allowsListing()`.
    pub fn allows_listing(&self) -> bool {
        self.allows_listing
    }

    /// `ClientInformation.particleStatus()`.
    pub fn particle_status(&self) -> ParticleStatus {
        self.particle_status
    }

    /// `ClientInformation.createDefault()` — `Player.DEFAULT_MAIN_HAND` is
    /// `HumanoidArm.RIGHT` (`Avatar.java`).
    pub fn create_default() -> Self {
        ClientInformation::new(
            "en_us".to_string(),
            2,
            ChatVisiblity::Full,
            true,
            0,
            HumanoidArm::Right,
            false,
            false,
            ParticleStatus::All,
        )
    }

    /// `ClientInformation.write(FriendlyByteBuf)` / `new
    /// ClientInformation(FriendlyByteBuf)`.
    pub fn stream_codec() -> StreamCodec<FriendlyByteBuf, ClientInformation> {
        let language_decode = byte_buf_codecs::string_utf8(MAX_LANGUAGE_LENGTH);
        of(
            move |output: &mut FriendlyByteBuf, value: &ClientInformation| {
                output.write_utf(&value.language);
                output.write_byte(value.view_distance);
                output.write_enum(ChatVisiblity::id, &value.chat_visibility);
                output.write_boolean(value.chat_colors);
                output.write_byte(value.model_customisation as i8);
                output.write_enum(HumanoidArm::id, &value.main_hand);
                output.write_boolean(value.text_filtering_enabled);
                output.write_boolean(value.allows_listing);
                output.write_enum(ParticleStatus::id, &value.particle_status);
                Ok(())
            },
            move |input: &mut FriendlyByteBuf| {
                let language = language_decode.decode(input)?;
                let view_distance = input.read_byte();
                let chat_visibility_id = input.read_var_int();
                let chat_visibility =
                    ChatVisiblity::from_id(chat_visibility_id).ok_or_else(|| {
                        CodecError::new(format!(
                            "Index {chat_visibility_id} out of bounds for length {}",
                            ChatVisiblity::COUNT
                        ))
                    })?;
                let chat_colors = input.read_boolean();
                let model_customisation = input.read_unsigned_byte();
                let main_hand_id = input.read_var_int();
                let main_hand = HumanoidArm::from_id(main_hand_id).ok_or_else(|| {
                    CodecError::new(format!(
                        "Index {main_hand_id} out of bounds for length {}",
                        HumanoidArm::COUNT
                    ))
                })?;
                let text_filtering_enabled = input.read_boolean();
                let allows_listing = input.read_boolean();
                let particle_status_id = input.read_var_int();
                let particle_status =
                    ParticleStatus::from_id(particle_status_id).ok_or_else(|| {
                        CodecError::new(format!(
                            "Index {particle_status_id} out of bounds for length {}",
                            ParticleStatus::COUNT
                        ))
                    })?;
                Ok(ClientInformation::new(
                    language,
                    view_distance,
                    chat_visibility,
                    chat_colors,
                    model_customisation,
                    main_hand,
                    text_filtering_enabled,
                    allows_listing,
                    particle_status,
                ))
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{CodecError, StreamEncoder};
    use bytes::BytesMut;

    fn encode(info: &ClientInformation) -> Vec<u8> {
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        ClientInformation::stream_codec()
            .encode(&mut out, info)
            .unwrap();
        out.into_inner().to_vec()
    }

    fn decode(bytes: &[u8]) -> Result<ClientInformation, CodecError> {
        let mut input = FriendlyByteBuf::new(BytesMut::from(bytes));
        ClientInformation::stream_codec().decode(&mut input)
    }

    fn round_trip(info: &ClientInformation) {
        assert_eq!(&decode(&encode(info)).unwrap(), info);
    }

    #[test]
    fn golden_wire_bytes_for_create_default() {
        // `createDefault()` traced through `ClientInformation.write`:
        //   varint 5 + "en_us", byte 2, varint 0 (FULL), bool true, byte 0,
        //   varint 1 (RIGHT), bool false, bool false, varint 0 (ALL).
        assert_eq!(
            encode(&ClientInformation::create_default()),
            vec![
                0x05, 0x65, 0x6e, 0x5f, 0x75, 0x73, // "en_us"
                0x02, // viewDistance 2
                0x00, // ChatVisiblity.FULL
                0x01, // chatColors true
                0x00, // modelCustomisation 0
                0x01, // HumanoidArm.RIGHT
                0x00, // textFilteringEnabled false
                0x00, // allowsListing false
                0x00, // ParticleStatus.ALL
            ]
        );
    }

    #[test]
    fn round_trips_every_enum_variant() {
        for visibility in [
            ChatVisiblity::Full,
            ChatVisiblity::System,
            ChatVisiblity::Hidden,
        ] {
            for main_hand in [HumanoidArm::Left, HumanoidArm::Right] {
                for particle_status in [
                    ParticleStatus::All,
                    ParticleStatus::Decreased,
                    ParticleStatus::Minimal,
                ] {
                    let info = ClientInformation::new(
                        "en_us".to_string(),
                        2,
                        visibility,
                        true,
                        0,
                        main_hand,
                        false,
                        false,
                        particle_status,
                    );
                    round_trip(&info);
                }
            }
        }
    }

    #[test]
    fn negative_view_distance_decodes_as_valid() {
        // Configuration stores even negative view distances (play listener
        // validation rejects them, issue #96); the codec must not clamp.
        let info = ClientInformation::new(
            "en_us".to_string(),
            -5,
            ChatVisiblity::Full,
            true,
            0,
            HumanoidArm::Right,
            false,
            false,
            ParticleStatus::All,
        );
        let bytes = encode(&info);
        // The signed byte -5 is 0xFB (field 2, after the language).
        assert_eq!(bytes[6], 0xFB);
        assert_eq!(decode(&bytes).unwrap().view_distance(), -5);
        // The full signed range round-trips.
        for v in [i8::MIN, -1, 0, 1, 127] {
            round_trip(&ClientInformation::new(
                "en_us".to_string(),
                v,
                ChatVisiblity::Full,
                true,
                0,
                HumanoidArm::Right,
                false,
                false,
                ParticleStatus::All,
            ));
        }
    }

    #[test]
    fn model_customisation_unsigned_mask_round_trips() {
        // `readUnsignedByte` yields 0..=255; the encode side writes the low 8
        // bits (`writeByte`), so 255 encodes as 0xFF and decodes back to 255.
        let info = ClientInformation::new(
            "en_us".to_string(),
            2,
            ChatVisiblity::Full,
            true,
            255,
            HumanoidArm::Right,
            false,
            false,
            ParticleStatus::All,
        );
        let bytes = encode(&info);
        assert_eq!(bytes[9], 0xFF);
        assert_eq!(decode(&bytes).unwrap().model_customisation(), 255);
        // A negative model mask encodes as 128 (bit 7 set) and decodes as 128.
        let info = ClientInformation::new(
            "en_us".to_string(),
            2,
            ChatVisiblity::Full,
            true,
            128,
            HumanoidArm::Right,
            false,
            false,
            ParticleStatus::All,
        );
        assert_eq!(encode(&info)[9], 0x80);
        assert_eq!(decode(&encode(&info)).unwrap().model_customisation(), 128);
    }

    #[test]
    fn language_at_exact_16_utf16_units_decodes() {
        // `readUtf(16)` accepts exactly 16 UTF-16 code units — the inclusive
        // boundary; 17 rejects (`language_over_16_utf16_units_errors_on_decode`).
        let info = ClientInformation::new(
            "abcdefghijklmnop".to_string(),
            2,
            ChatVisiblity::Full,
            true,
            0,
            HumanoidArm::Right,
            false,
            false,
            ParticleStatus::All,
        );
        round_trip(&info);
    }

    #[test]
    fn surrogate_pair_language_round_trips() {
        // 8 emoji (each a UTF-16 surrogate pair) is exactly 16 UTF-16 units but
        // 32 UTF-8 bytes — inside `utf8MaxBytes(16) = 48` yet exercising the
        // multibyte path at the packet level.
        let info = ClientInformation::new(
            "😀".repeat(8),
            2,
            ChatVisiblity::Full,
            true,
            0,
            HumanoidArm::Right,
            false,
            false,
            ParticleStatus::All,
        );
        round_trip(&info);
        // The length prefix is the UTF-8 byte count (32), not the unit count.
        assert_eq!(encode(&info)[0], 32);
    }

    #[test]
    fn encode_accepts_language_decode_rejects() {
        // The language boundary is asymmetric exactly as Java's: encode uses
        // `writeUtf` (max `MAX_STRING_LENGTH`), decode uses `readUtf(16)`, so a
        // 17-unit language encodes but decode later rejects it.
        let info = ClientInformation::new(
            "abcdefghijklmnopq".to_string(),
            2,
            ChatVisiblity::Full,
            true,
            0,
            HumanoidArm::Right,
            false,
            false,
            ParticleStatus::All,
        );
        let encoded = encode(&info); // encode side accepts 17 units
        let err = decode(&encoded).unwrap_err();
        assert_eq!(
            err.message,
            "The received string length is longer than maximum allowed (17 > 16)"
        );
    }

    #[test]
    fn language_over_16_utf16_units_errors_on_decode() {
        // `readUtf(16)` bounds by UTF-16 code units: 17 ASCII chars are 17
        // units, so decode rejects them (Java `DecoderException`), surfaced as
        // `Err`. The trailing fields are never reached.
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        out.write_var_int(17);
        out.write_bytes(&[b'a'; 17]);
        out.write_byte(2);
        out.write_var_int(0);
        out.write_boolean(true);
        out.write_byte(0);
        out.write_var_int(1);
        out.write_boolean(false);
        out.write_boolean(false);
        out.write_var_int(0);
        let err = decode(&out.into_inner()).unwrap_err();
        assert_eq!(
            err.message,
            "The received string length is longer than maximum allowed (17 > 16)"
        );
    }

    #[test]
    fn oversize_language_buffer_errors() {
        // `utf8MaxBytes(16)` is 48: a declared byte length over 48 is rejected
        // before the payload is touched.
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        out.write_var_int(60);
        out.write_bytes(&[b'a'; 60]);
        let err = decode(&out.into_inner()).unwrap_err();
        assert_eq!(
            err.message,
            "The received encoded string buffer length is longer than maximum allowed (60 > 48)"
        );
    }

    #[test]
    fn out_of_range_enum_ordinals_error() {
        // Each enum's `values()[id]` is an `ArrayIndexOutOfBoundsException`;
        // the codec surfaces the same `Index ... out of bounds for length ...`
        // message as `Err`. Build a valid prefix, then a bad ordinal in each
        // enum position.
        let mut chat = FriendlyByteBuf::new(BytesMut::new());
        chat.write_var_int(5);
        chat.write_bytes(b"en_us");
        chat.write_byte(2);
        chat.write_var_int(3); // ChatVisiblity ordinal 3 (COUNT 3)
        let err = decode(&chat.into_inner()).unwrap_err();
        assert_eq!(err.message, "Index 3 out of bounds for length 3");

        let mut arm = FriendlyByteBuf::new(BytesMut::new());
        arm.write_var_int(5);
        arm.write_bytes(b"en_us");
        arm.write_byte(2);
        arm.write_var_int(0);
        arm.write_boolean(true);
        arm.write_byte(0);
        arm.write_var_int(2); // HumanoidArm ordinal 2 (COUNT 2)
        let err = decode(&arm.into_inner()).unwrap_err();
        assert_eq!(err.message, "Index 2 out of bounds for length 2");

        let mut particle = FriendlyByteBuf::new(BytesMut::new());
        particle.write_var_int(5);
        particle.write_bytes(b"en_us");
        particle.write_byte(2);
        particle.write_var_int(0);
        particle.write_boolean(true);
        particle.write_byte(0);
        particle.write_var_int(1);
        particle.write_boolean(false);
        particle.write_boolean(false);
        particle.write_var_int(3); // ParticleStatus ordinal 3 (COUNT 3)
        let err = decode(&particle.into_inner()).unwrap_err();
        assert_eq!(err.message, "Index 3 out of bounds for length 3");
    }

    #[test]
    fn negative_enum_ordinal_errors() {
        // `getEnumConstants()[-1]` throws `ArrayIndexOutOfBoundsException` with
        // the same negative index; the codec surfaces it as `Err`. `write_var_int`
        // of -1 is the 5-byte Protocol VarInt encoding Java's `readVarInt` reads.
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        out.write_var_int(5);
        out.write_bytes(b"en_us");
        out.write_byte(2);
        out.write_var_int(-1); // ChatVisiblity ordinal -1
        let err = decode(&out.into_inner()).unwrap_err();
        assert_eq!(err.message, "Index -1 out of bounds for length 3");
    }

    #[test]
    fn trailing_bytes_after_particle_status_are_left_unread() {
        // `new ClientInformation(FriendlyByteBuf)` reads exactly the nine fields
        // and never checks for trailing bytes; the codec must stop after
        // `particleStatus` and leave any trailing garbage unread.
        let mut bytes = encode(&ClientInformation::create_default());
        bytes.extend_from_slice(&[0xAA, 0xBB]);
        let mut input = FriendlyByteBuf::new(BytesMut::from(bytes.as_slice()));
        let decoded = ClientInformation::stream_codec()
            .decode(&mut input)
            .unwrap();
        assert_eq!(decoded, ClientInformation::create_default());
        assert_eq!(input.readable_bytes(), 2);
    }

    #[test]
    fn create_default_matches_java() {
        let d = ClientInformation::create_default();
        assert_eq!(d.language(), "en_us");
        assert_eq!(d.view_distance(), 2);
        assert_eq!(d.chat_visibility(), ChatVisiblity::Full);
        assert!(d.chat_colors());
        assert_eq!(d.model_customisation(), 0);
        assert_eq!(d.main_hand(), HumanoidArm::Right);
        assert!(!d.text_filtering_enabled());
        assert!(!d.allows_listing());
        assert_eq!(d.particle_status(), ParticleStatus::All);
    }

    #[test]
    fn humanoid_arm_get_opposite() {
        assert_eq!(HumanoidArm::Left.get_opposite(), HumanoidArm::Right);
        assert_eq!(HumanoidArm::Right.get_opposite(), HumanoidArm::Left);
    }
}
