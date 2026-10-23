//! ClientboundAddEntityPacket and LpVec3 motion encoding (Paper 26.2).
//! Type IDs must be resolved from the actual owning world's registry.
use crate::{
    codec::StreamCodec,
    friendly_byte_buf::FriendlyByteBuf,
    protocol::{
        packet::{Packet, codec},
        packet_type::PacketType,
    },
    registry_friendly_byte_buf::RegistryFriendlyByteBuf,
};
use rivet_registry::core::Vec3;
use rivet_util::mth::Uuid;

#[derive(Debug, Clone, PartialEq)]
pub struct ClientboundAddEntityPacket {
    pub id: i32,
    pub uuid: Uuid,
    pub entity_type_id: i32,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub movement: Vec3,
    pub x_rot: i8,
    pub y_rot: i8,
    pub y_head_rot: i8,
    pub data: i32,
}
impl Packet for ClientboundAddEntityPacket {
    fn packet_type(&self) -> PacketType {
        PacketType::clientbound("add_entity")
    }
}
pub fn add_entity_codec() -> StreamCodec<RegistryFriendlyByteBuf, ClientboundAddEntityPacket> {
    codec(
        |value: &ClientboundAddEntityPacket, output: &mut RegistryFriendlyByteBuf| {
            let output = output.inner_mut();
            output.write_var_int(value.id);
            output.write_uuid(value.uuid);
            output.write_var_int(value.entity_type_id);
            output.write_double(value.x);
            output.write_double(value.y);
            output.write_double(value.z);
            write_lp_vec3(output, value.movement);
            output.write_byte(value.x_rot);
            output.write_byte(value.y_rot);
            output.write_byte(value.y_head_rot);
            output.write_var_int(value.data);
            Ok(())
        },
        |input: &mut RegistryFriendlyByteBuf| {
            let input = input.inner_mut();
            let id = input.read_var_int();
            let uuid = input.read_uuid();
            let raw_type = input.read_var_int();
            // BuiltInRegistries.ENTITY_TYPE is defaulted to pig. The protocol
            // boundary retains resolved built-in IDs, not unknown wire IDs.
            let entity_type_id = if raw_type >= 0
                && (raw_type as usize)
                    < rivet_registry::generated::registries::ENTITY_TYPE_BY_ID.len()
            {
                raw_type
            } else {
                *rivet_registry::generated::registries::ENTITY_TYPE_BY_NAME
                    .get("minecraft:pig")
                    .unwrap() as i32
            };
            Ok(ClientboundAddEntityPacket {
                id,
                uuid,
                entity_type_id,
                x: input.read_double(),
                y: input.read_double(),
                z: input.read_double(),
                movement: read_lp_vec3(input),
                x_rot: input.read_byte(),
                y_rot: input.read_byte(),
                y_head_rot: input.read_byte(),
                data: input.read_var_int(),
            })
        },
    )
}
/// Exact LpVec3: packed components, optional unsigned scale, zero shortcut.
pub fn write_lp_vec3(output: &mut FriendlyByteBuf, value: Vec3) {
    fn sanitize(v: f64) -> f64 {
        if v.is_nan() {
            0.
        } else {
            v.clamp(-1.7179869183E10, 1.7179869183E10)
        }
    }
    let [x, y, z] = [sanitize(value.x), sanitize(value.y), sanitize(value.z)];
    let length = x.abs().max(y.abs()).max(z.abs());
    if length < 3.051944088384301E-5 {
        output.write_byte(0);
        return;
    }
    let scale = length.ceil() as u64;
    let partial = (scale & 3) != scale;
    let markers = if partial { scale & 3 | 4 } else { scale };
    let pack = |v: f64| ((v / scale as f64 * 0.5 + 0.5) * 32766. + 0.5).floor() as u64;
    let bits = markers | pack(x) << 3 | pack(y) << 18 | pack(z) << 33;
    output.write_byte(bits as i8);
    output.write_byte((bits >> 8) as i8);
    output.write_int((bits >> 16) as i32);
    if partial {
        output.write_var_int((scale >> 2) as i32);
    }
}
pub fn read_lp_vec3(input: &mut FriendlyByteBuf) -> Vec3 {
    let lowest = u64::from(input.read_unsigned_byte());
    if lowest == 0 {
        return Vec3::new(0., 0., 0.);
    }
    let middle = u64::from(input.read_unsigned_byte());
    let highest = input.read_int() as u32 as u64;
    let bits = highest << 16 | middle << 8 | lowest;
    let mut scale = lowest & 3;
    if lowest & 4 != 0 {
        scale |= (input.read_var_int() as u32 as u64) << 2;
    }
    let unpack = |shift: u32| {
        (((bits >> shift) & 32767).min(32766) as f64 * 2. / 32766. - 1.) * scale as f64
    };
    Vec3::new(unpack(3), unpack(18), unpack(33))
}
