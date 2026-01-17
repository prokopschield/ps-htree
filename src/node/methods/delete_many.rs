use ps_hkey::Store;
use ps_uuid::UUID;

use crate::{HtreeKey, HtreeNode, LEAF_HEIGHT};

impl<T> HtreeNode<T> {
    /// Removes multiple items from the tree by their keys.
    ///
    /// This operation is **idempotent**: calling it multiple times with the same keys has the same
    /// effect as calling it once. Duplicate keys or keys not present in the tree are safely ignored.
    ///
    /// # Behavior
    /// - **Leaf level**: Filters out matching leaves directly.
    /// - **Internal nodes**: Recursively distributes deletion requests to child subtrees based on
    ///   key ranges, then rebuilds the node from resulting children.
    /// - **Empty result**: If all leaves are deleted and the tree becomes empty, returns an empty
    ///   node (`Default::default()`).
    /// - **Node contraction**: Never causes splits or height increases. May reduce tree height if
    ///   entire subtrees become empty.
    ///
    /// # Performance Characteristics
    /// - **Initial Setup**: O(K log K) for sorting and deduplicating the input key set.
    /// - **Traversal Time**: `O(N_visited * log K)` where `N_visited` is the total number of nodes in
    ///   subtrees affected by the deletion keys.
    /// - **Routing Complexity**: Each internal node uses binary search to partition the input
    ///   slice: O(C log K) per node, where C is the children per node.
    /// - **I/O Efficiency**: Only relevant subtrees are touched.
    ///
    /// # Errors
    /// This function can fail due to several error conditions:
    ///
    /// | Error | Cause | Recovery |
    /// |-------|-------|----------|
    /// | `CorruptedNode` | Invalid node state detected during child fetching | Use backup or recreate tree |
    /// | `Key(HtreeKeyError)` | Failed to convert input keys to UUIDs (store lookup failure, malformed keys) | Verify keys are valid, check store health |
    /// | `FromChildren(HtreeNodeFromChildrenError)` | Failed to reconstruct node after recursive deletion | See [`HtreeNode::from_children`][`crate::HtreeNode::from_children`] |
    /// | `Store(S::Error)` | Store fetch or put failed | Check whether you're using the Store that actually holds your data and that it's not full |
    /// | `UnpackChildren` | Failed to deserialize child nodes | The node you're using is probably malformed and possibly malicious. |
    ///
    /// # Returns
    /// A new `HtreeNode` representing the tree after deletions. Always succeeds with a valid node unless an error occurs.
    pub fn delete_many<'k, K, I, S>(
        &self,
        keys: I,
        store: &S,
    ) -> Result<Self, HtreeNodeDeleteManyError<S>>
    where
        K: HtreeKey + 'k,
        I: IntoIterator<Item = &'k K>,
        S: Store,
    {
        let mut uuids = keys
            .into_iter()
            .map(|k| k.try_to_uuid(store))
            .collect::<Result<Vec<UUID>, _>>()
            .map_err(HtreeNodeDeleteManyError::Key)?;

        if uuids.is_empty() {
            return Ok(self.clone());
        }

        uuids.sort_unstable();
        uuids.dedup();

        if self.is_leaf() {
            if uuids.binary_search(&self.key).is_ok() {
                return Ok(Self::default());
            }

            return Ok(self.clone());
        }

        let siblings = self.delete_leaves_recursive(&uuids, store)?;

        // Deletion cannot expand the number of nodes required to hold data.
        // It will usually return 1 node. If the tree became empty,
        // from_many_children returns an empty Vec, so we return Default.
        Ok(siblings.into_iter().next().unwrap_or_default())
    }

    /// Internal recursive helper for batch deletion.
    ///
    /// Distributes deletion keys to appropriate child subtrees and rebuilds from results.
    ///
    /// # Algorithm
    /// 1. **Leaf nodes**: Filter out matching leaves directly via `binary_search` on the input slice.
    /// 2. **Internal nodes**:
    ///    - Partition the sorted `keys_to_delete` slice into contiguous sub-slices based on
    ///      child boundaries using binary search (`partition_point`).
    ///    - Recurse only on children whose ranges contain one or more keys in the slice.
    ///    - Rebuild the node from resulting child nodes.
    ///
    /// # Returns
    /// `Vec<Self>` because internal node deletion can produce 0..N child nodes after merging.
    /// Single-node callers should use `into_iter().next().unwrap_or_default()`.
    ///
    /// # Performance Notes
    /// - **Partitioning**: O(C log K) where C is the child count and K is
    ///   the number of keys remaining in the current recursive branch.
    /// - **Search**: Utilizes binary search on the input slice rather than the children array,
    ///   leveraging the pre-sorted nature of the batch request.
    fn delete_leaves_recursive<S: Store>(
        &self,
        keys_to_delete: &[UUID],
        store: &S,
    ) -> Result<Vec<Self>, HtreeNodeDeleteManyError<S>> {
        // Base case: This node is the parent of leaf nodes
        if self.height <= LEAF_HEIGHT + 1 {
            let current_leaves = self.fetch_children(store)?;

            // Filter out matching leaves
            let filtered: Vec<Self> = current_leaves
                .into_iter()
                .filter(|leaf| {
                    // binary_search provides O(log K) complexity
                    keys_to_delete.binary_search(&leaf.key).is_err()
                })
                .collect();

            return Ok(Self::from_many_children(filtered, store)?);
        }

        let children = self.fetch_children(store)?;
        let mut rebuilt_children = Vec::with_capacity(children.len());

        // Efficiently slice keys_to_delete using the sliding pivot approach.
        // We ignore keys smaller than the first child's key immediately.
        let mut remaining_keys = children.first().map_or_else(
            || &[][..],
            |first| &keys_to_delete[keys_to_delete.partition_point(|&k| k < first.key)..],
        );

        let mut iter = children.into_iter().peekable();
        while let Some(node) = iter.next() {
            // Find the boundary: keys belonging to the current node are those
            // strictly less than the next sibling's key.
            let (to_process, rest) = iter.peek().map_or_else(
                // Last child consumes the remainder of relevant keys
                || (remaining_keys, &[][..]),
                |next_sibling| {
                    let mid = remaining_keys.partition_point(|&k| k < next_sibling.key);
                    remaining_keys.split_at(mid)
                },
            );

            if to_process.is_empty() {
                rebuilt_children.push(node);
            } else {
                let resulting_nodes = node.delete_leaves_recursive(to_process, store)?;

                rebuilt_children.extend(resulting_nodes);
            }

            remaining_keys = rest;
        }

        Ok(Self::from_many_children(rebuilt_children, store)?)
    }
}

/// Comprehensive error type for batch deletion operations.
///
/// This enum covers all failure modes from key conversion, node state validation,
/// child fetching, recursive deletion, and node reconstruction.
#[derive(thiserror::Error, Debug)]
pub enum HtreeNodeDeleteManyError<S: Store> {
    /// Node state is invalid or corrupted (detected during child fetching).
    ///
    /// Indicates structural invariants were violated. The node cannot be trusted.
    #[error("HtreeNode's state is corrupted.")]
    CorruptedNode,

    /// Failed to reconstruct valid node(s) from child nodes after recursive deletion.
    #[error("Node reconstruction failed.")]
    FromChildren(crate::HtreeNodeFromChildrenError<S>),

    /// Failed to convert input keys to UUIDs.
    ///
    /// This could imply the store isn't writable. See [`crate::HtreeKeyError`].
    #[error("Key error: {0}")]
    Key(#[from] crate::HtreeKeyError<S>),

    /// Underlying store operation failed.
    #[error("Store error: {0}")]
    Store(S::Error),

    /// Failed to deserialize child nodes from stored data.
    #[error("Error unpacking children: {0}")]
    UnpackChildren(#[from] crate::HtreeNodeUnpackChildrenError),
}

impl<S: Store> From<crate::HtreeNodeFetchChildrenError<S>> for HtreeNodeDeleteManyError<S> {
    fn from(value: crate::HtreeNodeFetchChildrenError<S>) -> Self {
        match value {
            crate::HtreeNodeFetchChildrenError::CorruptedState => Self::CorruptedNode,
            crate::HtreeNodeFetchChildrenError::Store(err) => Self::Store(err),
            crate::HtreeNodeFetchChildrenError::UnpackChildren(err) => Self::UnpackChildren(err),
        }
    }
}

impl<S: Store> From<crate::HtreeNodeFromChildrenError<S>> for HtreeNodeDeleteManyError<S> {
    fn from(value: crate::HtreeNodeFromChildrenError<S>) -> Self {
        match value {
            crate::HtreeNodeFromChildrenError::Store(err) => Self::Store(err),
            err => Self::FromChildren(err),
        }
    }
}
