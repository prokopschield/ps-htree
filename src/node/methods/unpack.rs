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
    /// # Empty Bytes
    ///
    /// If `bytes` is empty, this returns [`Self::default()`] (an empty node).
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
    /// - [`HtreeNodeUnpackError::UnpackChildren`] is returned if the byte sequence is malformed, truncated, or height is zero.
    /// - [`HtreeNodeUnpackError::Store`] is returned if store operations fail (from either `unpack_children` or `from_children`).
    /// - [`HtreeNodeUnpackError::FromChildren`] is returned if child reconstruction fails (e.g., height mismatch, overflow).
    pub fn unpack<S: Store>(bytes: &[u8], store: &S) -> Result<Self, HtreeNodeUnpackError<S>> {
        let children = Self::unpack_children(bytes)?;

        Ok(Self::from_children(children, store)?)
    }
}

#[derive(thiserror::Error, Debug)]
pub enum HtreeNodeUnpackError<S: Store> {
    #[error(transparent)]
    FromChildren(crate::HtreeNodeFromChildrenError<S>),
    #[error("Store error: {0}")]
    Store(S::Error),
    #[error(transparent)]
    UnpackChildren(#[from] HtreeNodeUnpackChildrenError),
}

impl<S: Store> From<crate::HtreeNodeFromChildrenError<S>> for HtreeNodeUnpackError<S> {
    fn from(value: crate::HtreeNodeFromChildrenError<S>) -> Self {
        match value {
            HtreeNodeFromChildrenError::Store(err) => Self::Store(err),
            err => Self::FromChildren(err),
        }
    }
}
