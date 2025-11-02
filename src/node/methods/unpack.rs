use ps_hash::HashValidationError;
use ps_hkey::{Hkey, Store};
use ps_rwt::RWT;
use ps_util::Array;
use ps_uuid::{UUID, UUID_BYTES};

use crate::{
    HtreeNode, LEAF_HEIGHT,
    node::{
        inner::{HtreeNodeReadonly, HtreeNodeWritable},
        methods::from_children::HtreeNodeFromChildrenError,
    },
};

impl<T> HtreeNode<T> {
    /// Deserializes an `HtreeNode` from its byte representation.
    ///
    /// This method reconstructs an `HtreeNode` from a compact byte encoding. The encoding format is:
    /// - First byte: height of the node
    /// - Remaining bytes: concatenated child nodes, each consisting of:
    ///   - UUID key (16 bytes)
    ///   - Child hkey length (1 byte)
    ///   - Child hkey (variable length)
    ///
    /// # Arguments
    ///
    /// * `bytes` - The serialized byte representation of the node. An empty slice yields a default node.
    /// * `store` - The store used to validate and reconstruct child nodes.
    ///
    /// # Returns
    ///
    /// Returns `Ok(HtreeNode)` if deserialization succeeds, or `Err(HtreeNodeUnpackError)` if:
    /// - The height byte is zero
    /// - The byte sequence is malformed or truncated
    /// - Hash validation fails
    /// - Store operations fail
    /// - Child node reconstruction fails
    ///
    /// # Errors
    ///
    /// See [`HtreeNodeUnpackError`] for possible error variants.
    pub fn unpack<S: Store>(bytes: &[u8], store: &S) -> Result<Self, HtreeNodeUnpackError<S>> {
        if bytes.is_empty() {
            return Ok(Self::default());
        }

        let Some(height) = bytes[0].checked_sub(1) else {
            return Err(HtreeNodeUnpackValidationError::HeightIsZero)?;
        };

        let mut children = Vec::new();

        let mut remaining = &bytes[1..];
        while !remaining.is_empty() {
            let key = UUID::from_bytes(
                *remaining
                    .subarray_checked(0)
                    .ok_or(HtreeNodeUnpackValidationError::UnexpectedEndOfInput)?,
            );

            let child_hkey_len = usize::from(
                *remaining
                    .get(UUID_BYTES)
                    .ok_or(HtreeNodeUnpackValidationError::UnexpectedEndOfInput)?,
            );

            let hkey = Hkey::from_compact(
                remaining
                    .get(UUID_BYTES + 1..UUID_BYTES + 1 + child_hkey_len)
                    .ok_or(HtreeNodeUnpackValidationError::UnexpectedEndOfInput)?,
            )
            .map_err(HtreeNodeUnpackValidationError::Hash)?;

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

        Ok(Self::from_children(children, store)?)
    }
}

#[derive(thiserror::Error, Debug)]
pub enum HtreeNodeUnpackError<S: Store> {
    #[error(transparent)]
    FromChildren(#[from] HtreeNodeFromChildrenError<S>),
    #[error("Store error: {0}")]
    Store(S::Error),
    #[error("Validation error: {0}")]
    Validation(#[from] HtreeNodeUnpackValidationError),
}

#[derive(thiserror::Error, Debug)]
pub enum HtreeNodeUnpackValidationError {
    #[error("Invalid Hash")]
    Hash(#[from] HashValidationError),
    #[error("The height of an inner node cannot be zero.")]
    HeightIsZero,
    #[error("Unexpected end of input")]
    UnexpectedEndOfInput,
}
