use ps_hkey::{DOUBLE_HASH_SIZE_COMPACT, Store};
use ps_rwt::RWT;
use ps_util::Array;
use ps_uuid::UUID_BYTES;

use crate::{
    HtreeNode,
    node::inner::{HtreeNodeReadonly, HtreeNodeWritable},
};

impl<T> HtreeNode<T> {
    /// Constructs a parent node from an iterator of child nodes.
    ///
    /// Sorts children, increments height, serializes them to the store, and
    /// returns a new internal node. If the iterator is empty, returns a
    /// default node.
    ///
    /// # Arguments
    ///
    /// * `children` - An iterator of child nodes.
    /// * `store` - The backing store for persisting node data.
    ///
    /// # Errors
    ///
    /// Returns [`HtreeNodeFromChildrenError`] if serialization or store
    /// operations fail.
    /// - [`HtreeNodeFromChildrenError::ChildHeightInconsistent`] is returned if not all of the children share the same height.
    /// - [`HtreeNodeFromChildrenError::HeightOverflow`] is returned if the node's height overflows (>255).
    pub fn from_children<I, S>(
        children: I,
        store: &S,
    ) -> Result<Self, HtreeNodeFromChildrenError<S>>
    where
        I: IntoIterator<Item = Self>,
        S: Store,
    {
        let mut children: Vec<Self> = children.into_iter().collect();

        children.sort();

        build_parent_from_sorted_children(children, store)
    }

    /// Constructs a parent node from children that are already sorted by key.
    ///
    /// Internal helper for callers that have already established ordering and
    /// want to avoid redundant sorting.
    pub(crate) fn from_sorted_children<I, S>(
        children: I,
        store: &S,
    ) -> Result<Self, HtreeNodeFromChildrenError<S>>
    where
        I: IntoIterator<Item = Self>,
        S: Store,
    {
        let children: Vec<Self> = children.into_iter().collect();

        build_parent_from_sorted_children(children, store)
    }
}

fn build_parent_from_sorted_children<T, S: Store>(
    children: Vec<HtreeNode<T>>,
    store: &S,
) -> Result<HtreeNode<T>, HtreeNodeFromChildrenError<S>> {
    if children.is_empty() {
        // return an empty node
        return Ok(HtreeNode::default());
    }

    let first_height = children[0].height;

    if children.some(|child| child.height != first_height) {
        return Err(HtreeNodeFromChildrenError::ChildHeightInconsistent);
    }

    let Some(height) = first_height.checked_add(1) else {
        return Err(HtreeNodeFromChildrenError::HeightOverflow);
    };

    let key = children[0].key;
    let hkey = store
        .put(&serialize_children(&children, store)?)
        .map_err(HtreeNodeFromChildrenError::Store)?;

    Ok(HtreeNode {
        inner: RWT::new(
            HtreeNodeReadonly { key, height, hkey },
            HtreeNodeWritable::Internal { children },
        ),
    })
}

/// Serializes child nodes into a buffer for storage.
///
/// Produces a buffer with the following layout:
/// - 1 byte: parent height (child height + 1)
/// - Per child:
///   - `UUID_BYTES`: child key
///   - 1 byte: compacted hkey length
///   - `0–DOUBLE_HASH_SIZE_COMPACT` bytes: compacted hkey (or verbatim short string)
///
/// Buffer capacity is pre-allocated as an upper bound based on maximum
/// compacted hkey size.
///
/// # Errors
///
/// Returns [`HtreeNodeFromChildrenError`] if hkey compaction or store
/// operations fail, or if hkey length exceeds a single byte.
fn serialize_children<T, S: Store>(
    children: &[HtreeNode<T>],
    store: &S,
) -> Result<Vec<u8>, HtreeNodeFromChildrenError<S>> {
    let length = 1 + children.len() * (UUID_BYTES + 1 + DOUBLE_HASH_SIZE_COMPACT);
    let mut buffer = Vec::with_capacity(length);

    buffer.push(children[0].height + 1);

    for child in children {
        buffer.extend_from_slice(child.key.as_bytes());

        let hkey = child
            .hkey
            .compact(store)
            .map_err(HtreeNodeFromChildrenError::Store)?;

        buffer.push(hkey.len().try_into()?);
        buffer.extend_from_slice(&hkey);
    }

    Ok(buffer)
}

/// Errors that can occur when constructing a node from children.
#[derive(thiserror::Error, Debug)]
pub enum HtreeNodeFromChildrenError<S: Store> {
    #[error("Child nodes must all have the same height.")]
    ChildHeightInconsistent,

    #[error("Maximum height of 255 exceeded.")]
    HeightOverflow,

    /// An integer conversion failed (e.g., hkey length exceeds byte range).
    #[error("Integer conversion error: {0}")]
    IntConv(#[from] std::num::TryFromIntError),

    /// A store operation failed.
    #[error("Store error: {0}")]
    Store(S::Error),
}
