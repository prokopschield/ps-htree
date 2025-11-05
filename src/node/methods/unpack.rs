use ps_hkey::Store;

use crate::{
    HtreeNode, HtreeNodeUnpackChildrenError,
    node::methods::from_children::HtreeNodeFromChildrenError,
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
    /// * `bytes` - The serialized byte representation of the node. An empty slice yields an empty node.
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
        let children = Self::unpack_children(bytes)?;

        Ok(Self::from_children(children, store)?)
    }
}

#[derive(thiserror::Error, Debug)]
pub enum HtreeNodeUnpackError<S: Store> {
    #[error(transparent)]
    FromChildren(#[from] HtreeNodeFromChildrenError<S>),
    #[error("Store error: {0}")]
    Store(S::Error),
    #[error(transparent)]
    UnpackChildren(#[from] HtreeNodeUnpackChildrenError),
}
