//! Fuzz target: the shared `ClientInformation` serverbound body codec
//! (issue #197).
//!
//! `ClientInformation` is registered at both configuration id 0 and play id 14
//! (pinned by the generated tables), so it is the one crossover serverbound
//! body. This target feeds arbitrary bytes straight into its `stream_codec`
//! decode: a bounded language string (`string_utf8(16)`), a view-distance byte,
//! three varint enum ordinals that return `Err` on an out-of-range id, three
//! bools, and a model-customisation byte. Short reads panic faithfully (EOF);
//! every other hostile shape resolves to `Err` or a faithful panic — anything
//! else aborts the fuzzer and writes an artifact.
#![no_main]
use bytes::BytesMut;
use libfuzzer_sys::fuzz_target;
use rivet_protocol::codec::StreamDecoder;
use rivet_protocol::friendly_byte_buf::FriendlyByteBuf;
use rivet_protocol::protocol::common::client_information::ClientInformation;

mod guard;
use guard::guarded;

fuzz_target!(|data: &[u8]| {
    if data.len() > guard::MAX_INPUT_LEN {
        return;
    }
    guarded(|| {
        let mut input = FriendlyByteBuf::new(BytesMut::from(data));
        let _ = ClientInformation::stream_codec().decode(&mut input);
    });
});
