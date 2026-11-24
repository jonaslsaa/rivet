//! Deterministic valid packet bodies, written straight onto the wire.
//!
//! Each seed is built with [`Wire`], a tiny writer for the protocol's scalar
//! encodings, following the Java stream codec's field order. They deliberately
//! do not go through the ported Rust codecs, so Rivet's decoders are exercised
//! against bytes they did not produce; Paper then decodes and re-encodes each
//! one, and a seed Paper declines is still a legitimate input (both sides must
//! then decline it).

use super::packet::Entry;
use super::packet_corpus::Rng;

/// A growable wire buffer with the `FriendlyByteBuf` scalar writers.
#[derive(Default)]
pub struct Wire(Vec<u8>);

impl Wire {
    pub fn new() -> Wire {
        Wire(Vec::new())
    }
    pub fn raw(mut self, bytes: &[u8]) -> Wire {
        self.0.extend_from_slice(bytes);
        self
    }
    pub fn u8(self, v: u8) -> Wire {
        self.raw(&[v])
    }
    pub fn bool(self, v: bool) -> Wire {
        self.u8(u8::from(v))
    }
    pub fn i16(self, v: i16) -> Wire {
        self.raw(&v.to_be_bytes())
    }
    pub fn i32(self, v: i32) -> Wire {
        self.raw(&v.to_be_bytes())
    }
    pub fn i64(self, v: i64) -> Wire {
        self.raw(&v.to_be_bytes())
    }
    pub fn f32(self, v: f32) -> Wire {
        self.raw(&v.to_be_bytes())
    }
    pub fn f64(self, v: f64) -> Wire {
        self.raw(&v.to_be_bytes())
    }
    pub fn varint(mut self, v: i32) -> Wire {
        let mut v = v as u32;
        loop {
            if v & !0x7f == 0 {
                self.0.push(v as u8);
                return self;
            }
            self.0.push((v & 0x7f) as u8 | 0x80);
            v >>= 7;
        }
    }
    pub fn varlong(mut self, v: i64) -> Wire {
        let mut v = v as u64;
        loop {
            if v & !0x7f == 0 {
                self.0.push(v as u8);
                return self;
            }
            self.0.push((v & 0x7f) as u8 | 0x80);
            v >>= 7;
        }
    }
    pub fn string(self, s: &str) -> Wire {
        self.varint(s.len() as i32).raw(s.as_bytes())
    }
    pub fn uuid(self, hi: i64, lo: i64) -> Wire {
        self.i64(hi).i64(lo)
    }
    pub fn byte_array(self, bytes: &[u8]) -> Wire {
        self.varint(bytes.len() as i32).raw(bytes)
    }
    /// `BlockPos.asLong`: x 26 bits, z 26 bits, y 12 bits.
    pub fn block_pos(self, x: i32, y: i32, z: i32) -> Wire {
        self.i64(
            ((i64::from(x) & 0x3ff_ffff) << 38)
                | ((i64::from(z) & 0x3ff_ffff) << 12)
                | (i64::from(y) & 0xfff),
        )
    }
    pub fn done(self) -> Vec<u8> {
        self.0
    }
}

fn pick<'a>(rng: &mut Rng, items: &[&'a str]) -> &'a str {
    items[rng.below(items.len() as u64) as usize]
}

fn small(rng: &mut Rng) -> i32 {
    rng.below(1 << 14) as i32
}

fn wide(rng: &mut Rng) -> i64 {
    rng.next_u64() as i64
}

fn real(rng: &mut Rng) -> f64 {
    (rng.below(2_000_000) as f64 - 1_000_000.0) / 64.0
}

fn angle(rng: &mut Rng) -> f32 {
    (rng.below(360) as f32) - 180.0
}

fn identifier(rng: &mut Rng) -> String {
    pick(
        rng,
        &[
            "minecraft:brand",
            "minecraft:overworld",
            "rivet:probe",
            "minecraft:the_nether",
        ],
    )
    .to_owned()
}

fn client_information(rng: &mut Rng) -> Vec<u8> {
    Wire::new()
        .string(pick(rng, &["en_us", "de_de", "pt_br"]))
        .u8(rng.below(32) as u8 + 2)
        .varint(rng.below(3) as i32)
        .bool(rng.below(2) == 0)
        .u8(rng.below(128) as u8)
        .varint(rng.below(2) as i32)
        .bool(rng.below(2) == 0)
        .bool(rng.below(2) == 0)
        .varint(rng.below(3) as i32)
        .done()
}

fn cookie_response(rng: &mut Rng) -> Vec<u8> {
    let payload = rng.some_bytes(12);
    let w = Wire::new().string(&identifier(rng));
    if rng.below(3) == 0 {
        w.bool(false).done()
    } else {
        w.bool(true).byte_array(&payload).done()
    }
}

fn resource_pack(rng: &mut Rng) -> Vec<u8> {
    Wire::new()
        .uuid(wide(rng), wide(rng))
        .varint(rng.below(8) as i32)
        .done()
}

fn optional_uuid(rng: &mut Rng) -> Vec<u8> {
    if rng.below(2) == 0 {
        Wire::new().bool(false).done()
    } else {
        Wire::new().bool(true).uuid(wide(rng), wide(rng)).done()
    }
}

fn position_move_rotation(rng: &mut Rng) -> Wire {
    Wire::new()
        .f64(real(rng))
        .f64(real(rng))
        .f64(real(rng))
        .f64(real(rng) / 100.0)
        .f64(real(rng) / 100.0)
        .f64(real(rng) / 100.0)
        .f32(angle(rng))
        .f32(angle(rng))
}

/// Valid wire bodies for one packet, built from `rng`. Packets whose body is
/// too structured to write by hand here (chunk data, tag tables, registry
/// contents, entity data) rely on the captured bodies and mutations of them.
pub fn seeds(entry: &Entry, rng: &mut Rng) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut more = |make: &mut dyn FnMut(&mut Rng) -> Vec<u8>, n: usize, rng: &mut Rng| {
        for _ in 0..n {
            out.push(make(rng));
        }
    };
    match (entry.state, entry.direction, entry.name) {
        // ---- status
        (_, _, "minecraft:status_request") => out.push(Vec::new()),
        (_, _, "minecraft:ping_request" | "minecraft:pong_response") => {
            more(&mut |r| Wire::new().i64(wide(r)).done(), 3, rng)
        }
        ("status", "clientbound", "minecraft:status_response") => out.push(
            Wire::new()
                .string(r#"{"description":{"text":"A Minecraft Server"},"players":{"max":20,"online":0},"version":{"name":"26.2","protocol":776}}"#)
                .done(),
        ),
        // ---- login
        ("login", "serverbound", "minecraft:hello") => more(
            &mut |r| {
                Wire::new()
                    .string(pick(r, &["Notch", "RivetProbe", "a", "Player_01"]))
                    .uuid(wide(r), wide(r))
                    .done()
            },
            4,
            rng,
        ),
        (_, _, "minecraft:login_acknowledged" | "minecraft:finish_configuration"
            | "minecraft:chunk_batch_start" | "minecraft:client_tick_end" | "minecraft:clear_dialog"
            | "minecraft:accept_code_of_conduct") => out.push(Vec::new()),
        (_, "serverbound", "minecraft:cookie_response") => more(&mut cookie_response, 4, rng),
        (_, "clientbound", "minecraft:cookie_request") => {
            more(&mut |r| Wire::new().string(&identifier(r)).done(), 2, rng)
        }
        ("login", "clientbound", "minecraft:login_finished") => more(
            &mut |r| {
                let w = Wire::new()
                    .uuid(wide(r), wide(r))
                    .string(pick(r, &["Notch", "RivetProbe", "Player_01"]));
                match r.below(3) {
                    0 => w.varint(0).done(),
                    1 => w.varint(1).string("textures").string("e30=").bool(false).done(),
                    _ => w
                        .varint(1)
                        .string("textures")
                        .string("e30=")
                        .bool(true)
                        .string("c2ln")
                        .done(),
                }
            },
            4,
            rng,
        ),
        ("login", "clientbound", "minecraft:login_compression") => {
            more(&mut |r| Wire::new().varint(small(r)).done(), 3, rng)
        }
        // ---- common (configuration + play)
        (_, "clientbound", "minecraft:keep_alive") | (_, "serverbound", "minecraft:keep_alive") => {
            more(&mut |r| Wire::new().i64(wide(r)).done(), 3, rng)
        }
        (_, "clientbound", "minecraft:ping") | (_, "serverbound", "minecraft:pong") => {
            more(&mut |r| Wire::new().i32(wide(r) as i32).done(), 3, rng)
        }
        (_, "clientbound", "minecraft:resource_pack_pop") => more(&mut optional_uuid, 3, rng),
        (_, "serverbound", "minecraft:resource_pack") => more(&mut resource_pack, 3, rng),
        (_, "clientbound", "minecraft:store_cookie") => more(
            &mut |r| {
                let payload = r.some_bytes(20);
                Wire::new().string(&identifier(r)).byte_array(&payload).done()
            },
            3,
            rng,
        ),
        (_, "clientbound", "minecraft:transfer") => more(
            &mut |r| {
                Wire::new()
                    .string(pick(r, &["localhost", "example.org", "10.0.0.7"]))
                    .varint(r.below(65536) as i32)
                    .done()
            },
            3,
            rng,
        ),
        (_, "serverbound", "minecraft:client_information") => more(&mut client_information, 4, rng),
        ("configuration", "serverbound", "minecraft:custom_payload") => more(
            &mut |r| {
                if r.below(2) == 0 {
                    Wire::new().string("minecraft:brand").string("vanilla").done()
                } else {
                    let payload = r.some_bytes(16);
                    Wire::new().string("rivet:probe").raw(&payload).done()
                }
            },
            4,
            rng,
        ),
        ("configuration", "clientbound", "minecraft:custom_payload") => more(
            &mut |r| {
                if r.below(2) == 0 {
                    Wire::new().string("minecraft:brand").string("Paper").done()
                } else {
                    let payload = r.some_bytes(16);
                    Wire::new().string("rivet:probe").raw(&payload).done()
                }
            },
            4,
            rng,
        ),
        (_, _, "minecraft:select_known_packs") => more(
            &mut |_| Wire::new().varint(1).string("minecraft").string("core").string("26.2").done(),
            2,
            rng,
        ),
        ("configuration", "clientbound", "minecraft:custom_report_details") | ("play", "clientbound", "minecraft:custom_report_details") => more(
            &mut |r| {
                Wire::new()
                    .varint(1)
                    .string(pick(r, &["Server", "Region"]))
                    .string("rivet")
                    .done()
            },
            2,
            rng,
        ),
        ("configuration", "clientbound", "minecraft:update_enabled_features") => more(
            &mut |r| {
                Wire::new()
                    .varint(2)
                    .string("minecraft:vanilla")
                    .string(pick(r, &["minecraft:trade_rebalance", "minecraft:redstone_experiments"]))
                    .done()
            },
            2,
            rng,
        ),
        (_, "serverbound", "minecraft:custom_click_action") => more(
            &mut |r| {
                let w = Wire::new().string(&identifier(r));
                // `Optional<Tag>` has no presence flag (an absent tag is TAG_End)
                // and the tag is length-prefixed (`ByteBufCodecs.lengthPrefixed`).
                match r.below(3) {
                    0 => w.byte_array(&[0]).done(),
                    1 => w.byte_array(&[10, 0]).done(),
                    _ => w.byte_array(&[8, 0, 3, b'a', b'b', b'c']).done(),
                }
            },
            4,
            rng,
        ),
        (_, "clientbound", "minecraft:disconnect") => more(
            &mut |r| match r.below(3) {
                // an unnamed root TAG_String, the plain-text component form
                0 => Wire::new().u8(8).i16(5).raw(b"hello").done(),
                1 => Wire::new().u8(8).i16(12).raw(b"Kicked by Rivet").done(),
                // a TAG_Compound { text: "hi", bold: 1b }
                _ => Wire::new()
                    .u8(10)
                    .u8(8)
                    .i16(4)
                    .raw(b"text")
                    .i16(2)
                    .raw(b"hi")
                    .u8(1)
                    .i16(4)
                    .raw(b"bold")
                    .u8(1)
                    .u8(0)
                    .done(),
            },
            4,
            rng,
        ),
        (_, "clientbound", "minecraft:server_links") => more(
            &mut |r| match r.below(2) {
                0 => Wire::new()
                    .varint(1)
                    .bool(true)
                    .varint(r.below(10) as i32)
                    .string("https://example.org/rules")
                    .done(),
                _ => Wire::new()
                    .varint(1)
                    .bool(false)
                    .u8(8)
                    .i16(4)
                    .raw(b"Site")
                    .string("https://example.org")
                    .done(),
            },
            3,
            rng,
        ),
        (_, "clientbound", "minecraft:update_tags") => more(
            &mut |r| {
                Wire::new()
                    .varint(1)
                    .string("minecraft:item")
                    .varint(1)
                    .string("rivet:probe_tag")
                    .varint(2)
                    .varint(r.below(100) as i32)
                    .varint(r.below(100) as i32)
                    .done()
            },
            3,
            rng,
        ),
        // ---- play clientbound
        ("play", "clientbound", "minecraft:block_update") => more(
            &mut |r| {
                Wire::new()
                    .block_pos((small(r) % 1000) - 500, (small(r) % 300) - 60, (small(r) % 1000) - 500)
                    .varint(r.below(30000) as i32)
                    .done()
            },
            3,
            rng,
        ),
        ("play", "clientbound", "minecraft:change_difficulty") => {
            more(&mut |r| Wire::new().u8(r.below(5) as u8).bool(r.below(2) == 0).done(), 3, rng)
        }
        ("play", "clientbound", "minecraft:chunk_batch_finished" | "minecraft:set_chunk_cache_radius"
            | "minecraft:set_simulation_distance" | "minecraft:set_held_slot") => {
            more(&mut |r| Wire::new().varint(r.below(40) as i32).done(), 3, rng)
        }
        ("play", "clientbound", "minecraft:set_chunk_cache_center") => {
            more(&mut |r| Wire::new().varint(small(r) - 8192).varint(small(r) - 8192).done(), 3, rng)
        }
        ("play", "clientbound", "minecraft:entity_event") => {
            more(&mut |r| Wire::new().i32(small(r)).u8(r.below(64) as u8).done(), 3, rng)
        }
        ("play", "clientbound", "minecraft:game_event") => {
            more(&mut |r| Wire::new().u8(r.below(14) as u8).f32(r.below(5) as f32).done(), 3, rng)
        }
        ("play", "clientbound", "minecraft:initialize_border") => more(
            &mut |r| {
                Wire::new()
                    .f64(real(r))
                    .f64(real(r))
                    .f64(60_000_000.0)
                    .f64(60_000_000.0)
                    .varlong(0)
                    .varint(29_999_984)
                    .varint(5)
                    .varint(15)
                    .done()
            },
            2,
            rng,
        ),
        ("play", "clientbound", "minecraft:level_event") => more(
            &mut |r| {
                Wire::new()
                    .i32(1000 + r.below(3000) as i32)
                    .block_pos(small(r) % 500, small(r) % 200, small(r) % 500)
                    .i32(small(r))
                    .bool(r.below(2) == 0)
                    .done()
            },
            3,
            rng,
        ),
        ("play", "clientbound", "minecraft:player_abilities") => more(
            &mut |r| Wire::new().u8(r.below(16) as u8).f32(0.05).f32(0.1).done(),
            3,
            rng,
        ),
        ("play", "clientbound", "minecraft:player_info_remove") => more(
            &mut |r| Wire::new().varint(2).uuid(wide(r), wide(r)).uuid(wide(r), wide(r)).done(),
            2,
            rng,
        ),
        ("play", "clientbound", "minecraft:player_position") => more(
            &mut |r| {
                let w = Wire::new().varint(small(r));
                let pmr = position_move_rotation(r).done();
                w.raw(&pmr).i32(r.below(1 << 10) as i32).done()
            },
            3,
            rng,
        ),
        ("play", "clientbound", "minecraft:section_blocks_update") => more(
            &mut |r| {
                let pos = (i64::from(r.below(1 << 20) as u32) << 42)
                    | (i64::from(r.below(1 << 10) as u32) << 22)
                    | i64::from(r.below(1 << 20) as u32);
                Wire::new()
                    .i64(pos)
                    .varint(2)
                    .varlong((r.below(20000) as i64) << 12 | r.below(4096) as i64)
                    .varlong((r.below(20000) as i64) << 12 | r.below(4096) as i64)
                    .done()
            },
            3,
            rng,
        ),
        ("play", "clientbound", "minecraft:set_default_spawn_position") => more(
            &mut |r| {
                Wire::new()
                    .string(&identifier(r))
                    .block_pos(small(r) % 500, small(r) % 200, small(r) % 500)
                    .f32(angle(r))
                    .f32(angle(r) / 2.0)
                    .done()
            },
            3,
            rng,
        ),
        ("play", "clientbound", "minecraft:add_entity") => more(
            &mut |r| {
                Wire::new()
                    .varint(small(r))
                    .uuid(wide(r), wide(r))
                    .varint(r.below(150) as i32)
                    .f64(real(r))
                    .f64(real(r))
                    .f64(real(r))
                    .u8(0) // a zero movement vector is one byte
                    .u8(r.below(256) as u8)
                    .u8(r.below(256) as u8)
                    .u8(r.below(256) as u8)
                    .varint(small(r))
                    .done()
            },
            3,
            rng,
        ),
        ("play", "clientbound", "minecraft:chunks_biomes") => more(
            &mut |r| {
                let buffer = r.some_bytes(24);
                Wire::new()
                    .varint(1)
                    .i64(wide(r))
                    .byte_array(&buffer)
                    .done()
            },
            3,
            rng,
        ),
        ("play", "clientbound", "minecraft:light_update") => more(
            &mut |r| {
                // x, z, four empty BitSets, then empty sky and block update lists
                Wire::new()
                    .varint(small(r) - 8192)
                    .varint(small(r) - 8192)
                    .varint(0)
                    .varint(0)
                    .varint(0)
                    .varint(0)
                    .varint(0)
                    .varint(0)
                    .done()
            },
            2,
            rng,
        ),
        // ---- play serverbound
        ("play", "serverbound", "minecraft:accept_teleportation") => {
            more(&mut |r| Wire::new().varint(small(r)).done(), 3, rng)
        }
        ("play", "serverbound", "minecraft:chunk_batch_received") => {
            more(&mut |r| Wire::new().f32(r.below(100) as f32 / 4.0).done(), 3, rng)
        }
        ("play", "serverbound", "minecraft:client_command") => {
            more(&mut |r| Wire::new().varint(r.below(3) as i32).done(), 3, rng)
        }
        ("play", "serverbound", "minecraft:move_player_pos") => more(
            &mut |r| Wire::new().f64(real(r)).f64(real(r)).f64(real(r)).u8(r.below(4) as u8).done(),
            3,
            rng,
        ),
        ("play", "serverbound", "minecraft:move_player_pos_rot") => more(
            &mut |r| {
                Wire::new()
                    .f64(real(r))
                    .f64(real(r))
                    .f64(real(r))
                    .f32(angle(r))
                    .f32(angle(r) / 2.0)
                    .u8(r.below(4) as u8)
                    .done()
            },
            3,
            rng,
        ),
        ("play", "serverbound", "minecraft:move_player_rot") => more(
            &mut |r| Wire::new().f32(angle(r)).f32(angle(r) / 2.0).u8(r.below(4) as u8).done(),
            3,
            rng,
        ),
        ("play", "serverbound", "minecraft:move_player_status_only") => {
            more(&mut |r| Wire::new().u8(r.below(4) as u8).done(), 3, rng)
        }
        ("play", "serverbound", "minecraft:player_action") => more(
            &mut |r| {
                Wire::new()
                    .varint(r.below(7) as i32)
                    .block_pos(small(r) % 500, small(r) % 200, small(r) % 500)
                    .u8(r.below(6) as u8)
                    .varint(small(r))
                    .done()
            },
            3,
            rng,
        ),
        _ => {}
    }
    out
}
