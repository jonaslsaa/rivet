//! Paper 26.2 world identity lives in dimension data/paper/metadata.dat.
//! This read-only boundary does not generate a replacement UUID on failure:
//! the boot owner must explicitly create and persist a new world's identity.
use rivet_nbt::{nbt_accounter::NbtAccounter, nbt_io, nbt_ops::NbtOps};
use rivet_registry::core::uuid_codec;
use rivet_serialization::Dynamic;
use rivet_util::{DataInputStream, mth::Uuid};
use std::{io, path::Path};

/// Read the dimension's genuine saved UUID. DataFixTypes.NONE means no data
/// migration is applied. SavedDataStorage accepts both gzip and plain NBT,
/// and keeps a codec partial while reporting its diagnostic.
pub fn read_world_uuid(dimension_path: &Path) -> io::Result<Option<Uuid>> {
    let path = dimension_path.join("data/paper/metadata.dat");
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut accounter = NbtAccounter::unlimited_heap();
    let tag = if bytes.starts_with(&[0x1f, 0x8b]) {
        nbt_io::read_compressed(&bytes[..], &mut accounter)?
    } else {
        nbt_io::read(&mut DataInputStream::new(&bytes[..]), &mut accounter)?
    };
    let value = tag
        .get_compound("data")
        .and_then(|data| data.get("uuid"))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "Paper world metadata has no data.uuid",
            )
        })?;
    let ops = NbtOps::instance();
    let result = Dynamic::new(&ops, value.clone()).decode(&ops, &*uuid_codec::<NbtOps>());
    let uuid = result.result_or_partial(|message| {
        tracing::warn!(path = %path.display(), %message, "Paper world metadata UUID codec diagnostic");
    }).map(|(uuid, _)| uuid);
    uuid.map(Some).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "Paper world metadata UUID cannot be decoded",
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_nbt::{compound_tag::CompoundTag, tag::Tag};
    use rivet_util::DataOutputStream;

    #[test]
    fn reads_saved_identity_in_both_formats_and_preserves_codec_partial() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("data/paper");
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("metadata.dat");
        assert_eq!(read_world_uuid(root.path()).unwrap(), None);
        for compressed in [false, true] {
            for ints in [vec![0, -1, i32::MIN, 9], vec![0, -1, i32::MIN, 9, 77]] {
                let mut data = CompoundTag::new();
                data.put_int_array("uuid", ints);
                let mut root_tag = CompoundTag::new();
                root_tag.put("data".into(), Tag::Compound(data));
                root_tag.put_int("DataVersion", 4903);
                let mut bytes = Vec::new();
                if compressed {
                    nbt_io::write_compressed(&root_tag, &mut bytes).unwrap();
                } else {
                    nbt_io::write(&root_tag, &mut DataOutputStream::new(&mut bytes)).unwrap();
                }
                std::fs::write(&path, bytes).unwrap();
                assert_eq!(
                    read_world_uuid(root.path()).unwrap(),
                    Some(Uuid {
                        most: 0xffff_ffff,
                        least: i64::MIN + 9
                    })
                );
            }
        }
        let mut data = CompoundTag::new();
        data.put_int_array("uuid", vec![0, -1, i32::MIN]);
        let mut root_tag = CompoundTag::new();
        root_tag.put("data".into(), Tag::Compound(data));
        let mut bytes = Vec::new();
        nbt_io::write_compressed(&root_tag, &mut bytes).unwrap();
        std::fs::write(&path, bytes).unwrap();
        assert!(read_world_uuid(root.path()).is_err());
        std::fs::write(&path, b"invalid").unwrap();
        assert!(read_world_uuid(root.path()).is_err());
    }
}
