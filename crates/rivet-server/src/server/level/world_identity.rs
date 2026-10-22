//! Paper 26.2 world identity lives in dimension data/paper/metadata.dat.
//! Read-only imports never create identity. Writable boot explicitly invokes
//! Paper's load-or-create policy and persists the resulting metadata.
use rivet_nbt::{
    compound_tag::CompoundTag, nbt_accounter::NbtAccounter, nbt_io, nbt_ops::NbtOps, tag::Tag,
};
use rivet_registry::core::{uuid_codec, uuid_to_int_array};
use rivet_serialization::Dynamic;
use rivet_util::{DataInputStream, mth::Uuid};
use std::{io, path::Path};

/// PaperWorldLoader.loadWorldData: unavailable/corrupt metadata is logged and
/// replaced with UUID.randomUUID(), not a seed-derived or entity-LCG UUID.
/// The caller must own the dimension's writable world-storage lock.
pub fn load_or_create_world_uuid(dimension_path: &Path) -> io::Result<Uuid> {
    match std::panic::catch_unwind(|| read_world_uuid(dimension_path)) {
        Ok(Ok(Some(uuid))) => return Ok(uuid),
        Ok(Ok(None)) => {}
        Ok(Err(error)) => {
            tracing::error!(path = %dimension_path.display(), %error, "Error loading Paper world metadata")
        }
        Err(_) => {
            tracing::error!(path = %dimension_path.display(), "Unchecked NBT failure loading Paper world metadata")
        }
    }
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|error| io::Error::other(error.to_string()))?;
    // java.util.UUID.randomUUID uses secure bytes and these version/variant
    // masks before constructing its two big-endian signed long halves.
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Ok(Uuid {
        most: i64::from_be_bytes(bytes[..8].try_into().expect("UUID upper half")),
        least: i64::from_be_bytes(bytes[8..].try_into().expect("UUID lower half")),
    })
}

/// SavedDataStorage's paper:metadata encoding and gzip write. UUID is already
/// chosen by the boot owner; retries never generate another identity. Errors
/// are returned to that owner instead of claiming that persistence succeeded.
pub fn save_world_uuid(dimension_path: &Path, uuid: Uuid) -> io::Result<()> {
    let mut data = CompoundTag::new();
    data.put_int_array("uuid", uuid_to_int_array(uuid).to_vec());
    let mut root = CompoundTag::new();
    root.put("data".into(), Tag::Compound(data));
    root.put_int("DataVersion", super::region_backed::EXPECTED_DATA_VERSION);
    let directory = dimension_path.join("data/paper");
    std::fs::create_dir_all(&directory)?;
    let file = std::fs::File::create(directory.join("metadata.dat"))?;
    nbt_io::write_compressed(&root, file)
}

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
    fn writable_boot_creates_secure_identity_and_preserves_it_across_reload() {
        let root = tempfile::tempdir().unwrap();
        let uuid = load_or_create_world_uuid(root.path()).unwrap();
        assert_eq!((uuid.most as u64 >> 12) & 15, 4);
        assert_eq!((uuid.least as u64 >> 62) & 3, 2);
        assert_eq!(
            read_world_uuid(root.path()).unwrap(),
            None,
            "selection does not implicitly write"
        );
        save_world_uuid(root.path(), uuid).unwrap();
        assert_eq!(load_or_create_world_uuid(root.path()).unwrap(), uuid);
        let bytes = std::fs::read(root.path().join("data/paper/metadata.dat")).unwrap();
        let tag = nbt_io::read_compressed(&bytes[..], &mut NbtAccounter::unlimited_heap()).unwrap();
        assert_eq!(
            tag.get_int("DataVersion"),
            Some(super::super::region_backed::EXPECTED_DATA_VERSION)
        );
        assert_eq!(
            tag.get_compound("data")
                .unwrap()
                .get_int_array("uuid")
                .unwrap(),
            &uuid_to_int_array(uuid)
        );

        // SavedDataStorage catches invalid metadata; the writable loader then
        // chooses a replacement. Its eventual save makes subsequent boots stable.
        std::fs::write(root.path().join("data/paper/metadata.dat"), b"invalid").unwrap();
        let replacement = load_or_create_world_uuid(root.path()).unwrap();
        save_world_uuid(root.path(), replacement).unwrap();
        assert_eq!(read_world_uuid(root.path()).unwrap(), Some(replacement));
        let blocked = tempfile::NamedTempFile::new().unwrap();
        assert!(save_world_uuid(blocked.path(), replacement).is_err());
    }

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
