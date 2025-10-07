use ps_hkey::{Hkey, Store};
use ps_uuid::{UUID, UUID_BYTES};

use crate::{HtreeKey, HtreeKeyError};

impl HtreeKey for Hkey {
    fn try_to_hkey<S: Store>(&self, _: &S) -> Result<Hkey, HtreeKeyError<S>> {
        Ok(self.clone())
    }

    #[allow(clippy::cast_possible_truncation)]
    fn try_to_uuid<S: Store>(&self, store: &S) -> Result<UUID, HtreeKeyError<S>> {
        let input = self.compact(store).map_err(HtreeKeyError::Store)?;

        let copy_length = input.len().min(UUID_BYTES);

        let mut uuid = UUID::nil();

        uuid.as_mut_bytes()[0..copy_length].copy_from_slice(&input[..copy_length]);

        let fold = &mut uuid.as_mut_bytes()[8..16];

        for (chunk_index, chunk) in input.chunks(8).skip(2).enumerate() {
            fold.rotate_right(chunk_index & 7);

            for (byte_index, byte) in chunk.iter().enumerate() {
                fold[byte_index] ^= *byte;
            }
        }

        Ok(uuid.with_version(8))
    }
}
