use ps_uuid::{UUID, UUID_BYTES};

pub fn compact_to_uuid(bytes: &[u8]) -> UUID {
    let mut uuid = UUID::nil();

    let range = ..bytes.len().min(UUID_BYTES);
    uuid.as_mut_bytes()[range].copy_from_slice(&bytes[range]);

    let fold = &mut uuid.as_mut_bytes()[0x8..0x10];

    for (index, slice) in bytes.chunks(0x8).skip(2).enumerate() {
        fold.rotate_left(index);

        for (byte_index, &byte) in slice.iter().enumerate() {
            fold[byte_index] ^= byte;
        }
    }

    uuid.with_version(8)
}
