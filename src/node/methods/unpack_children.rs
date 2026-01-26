use ps_hkey::Hkey;
use ps_rwt::RWT;
use ps_util::Array;
use ps_uuid::{UUID, UUID_BYTES};

use crate::{
    HtreeNode, LEAF_HEIGHT,
    node::inner::{HtreeNodeReadonly, HtreeNodeWritable},
};

impl<T> HtreeNode<T> {
    /// Deserializes a `Vec<HtreeNode>` from its byte representation.
    ///
    /// This method reconstructs child nodes from a compact byte encoding. The encoding format is:
    /// - First byte: height of the node
    /// - Remaining bytes: concatenated child nodes, each consisting of:
    ///   - UUID key (16 bytes)
    ///   - Child hkey length (1 byte)
    ///   - Child hkey (variable length)
    ///
    /// # Arguments
    ///
    /// * `bytes` - The serialized byte representation of the node. An empty slice yields an empty list.
    ///
    /// # Returns
    ///
    /// Returns `Ok(Vec<HtreeNode>)` if deserialization succeeds, or `Err(HtreeNodeUnpackChildrenError)` if:
    /// - The height byte is zero
    /// - The byte sequence is malformed or truncated
    /// - Hash validation fails
    ///
    /// # Errors
    ///
    /// See [`HtreeNodeUnpackChildrenError`] for possible error variants.
    pub fn unpack_children(bytes: &[u8]) -> Result<Vec<Self>, HtreeNodeUnpackChildrenError> {
        if bytes.is_empty() {
            return Ok(Vec::new());
        }

        let Some(height) = bytes[0].checked_sub(1) else {
            return Err(HtreeNodeUnpackChildrenError::HeightIsZero)?;
        };

        let mut children = Vec::new();

        let mut remaining = &bytes[1..];
        while !remaining.is_empty() {
            let key = UUID::from_bytes(
                *remaining
                    .subarray_checked(0)
                    .ok_or(HtreeNodeUnpackChildrenError::UnexpectedEndOfInput)?,
            );

            let child_hkey_len = usize::from(
                *remaining
                    .get(UUID_BYTES)
                    .ok_or(HtreeNodeUnpackChildrenError::UnexpectedEndOfInput)?,
            );

            let hkey = Hkey::from_compact(
                remaining
                    .get(UUID_BYTES + 1..UUID_BYTES + 1 + child_hkey_len)
                    .ok_or(HtreeNodeUnpackChildrenError::UnexpectedEndOfInput)?,
            )
            .map_err(HtreeNodeUnpackChildrenError::HkeyFromCompact)?;

            remaining = &remaining[UUID_BYTES + 1 + child_hkey_len..];

            children.push(Self {
                inner: RWT::new(
                    HtreeNodeReadonly { key, height, hkey },
                    if height == LEAF_HEIGHT {
                        HtreeNodeWritable::Leaf
                    } else {
                        HtreeNodeWritable::Wrapped
                    },
                ),
            });
        }

        Ok(children)
    }
}

#[derive(thiserror::Error, Debug)]
pub enum HtreeNodeUnpackChildrenError {
    #[error("Invalid Hash")]
    HkeyFromCompact(#[from] ps_hkey::HkeyFromCompactError),
    #[error("The height of an inner node cannot be zero.")]
    HeightIsZero,
    #[error("Unexpected end of input")]
    UnexpectedEndOfInput,
}
