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
        K: HtreeKey + ?Sized + 'k,
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

        // Skip keys smaller than the first child's key, which can't exist in this subtree.
        let mut remaining_keys = children.first().map_or_else(
            || &[][..],
            |first| &keys_to_delete[keys_to_delete.partition_point(|&k| k < first.key)..],
        );

        let mut iter = children.into_iter().peekable();
        while let Some(child) = iter.next() {
            let next_key = iter.peek().map(|next| next.key);

            // Consecutive siblings with the same key only occur when
            // `from_many_children` split a duplicate run across child nodes.
            // That means this subtree contains only `child.key`, so we can
            // keep or drop it without recursing.
            if next_key == Some(child.key) {
                if remaining_keys.binary_search(&child.key).is_err() {
                    rebuilt_children.push(child);
                }

                continue;
            }

            // Partition: keys in [child.key, next_key) belong to this child.
            let split = next_key.map_or(remaining_keys.len(), |upper| {
                remaining_keys.partition_point(|&k| k < upper)
            });

            let (keys_for_child, rest) = remaining_keys.split_at(split);

            remaining_keys = rest;

            if keys_for_child.is_empty() {
                rebuilt_children.push(child);
            } else {
                rebuilt_children.extend(child.delete_leaves_recursive(keys_for_child, store)?);
            }
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

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use std::collections::HashSet;

    use ps_hkey::InMemoryStore;
    use ps_uuid::UUID;

    use crate::{HtreeNode, MAX_CHILDREN};

    fn collapse_to_root(mut roots: Vec<HtreeNode<u64>>, store: &InMemoryStore) -> HtreeNode<u64> {
        if roots.is_empty() {
            return HtreeNode::default();
        }

        while roots.len() > 1 {
            roots = HtreeNode::from_many_children(roots, store)
                .expect("from_many_children should succeed");
        }

        roots
            .into_iter()
            .next()
            .expect("non-empty roots should contain one node")
    }

    fn tree_from_keys(keys: &[UUID], store: &InMemoryStore) -> HtreeNode<u64> {
        if keys.is_empty() {
            return HtreeNode::default();
        }

        let leaves: Vec<_> = keys
            .iter()
            .enumerate()
            .map(|(idx, key)| {
                HtreeNode::from_kvp(key, &(idx as u64), store).expect("from_kvp should create leaf")
            })
            .collect();

        let roots = HtreeNode::from_many_children(leaves, store)
            .expect("from_many_children should succeed");
        collapse_to_root(roots, store)
    }

    fn collect_sorted_keys(tree: &HtreeNode<u64>, store: &InMemoryStore) -> Vec<UUID> {
        let mut keys: Vec<_> = tree
            .iter_keys(store)
            .map(|item| item.expect("iter_keys should succeed"))
            .collect();
        keys.sort_unstable();
        keys
    }

    fn key_occurrence_count(tree: &HtreeNode<u64>, key: UUID, store: &InMemoryStore) -> usize {
        tree.iter_keys(store)
            .map(|item| item.expect("iter_keys should succeed"))
            .filter(|candidate| *candidate == key)
            .count()
    }

    fn unique_sorted(mut keys: Vec<UUID>) -> Vec<UUID> {
        keys.sort_unstable();
        keys.dedup();
        keys
    }

    fn missing_keys(existing: &[UUID], count: usize) -> Vec<UUID> {
        let existing_set: HashSet<_> = existing.iter().copied().collect();
        let mut missing = Vec::with_capacity(count);
        while missing.len() < count {
            let candidate = UUID::gen_v4();
            if existing_set.contains(&candidate) || missing.contains(&candidate) {
                continue;
            }
            missing.push(candidate);
        }
        missing
    }

    fn patterned_keys(total: usize, distinct: usize) -> Vec<UUID> {
        if total == 0 {
            return Vec::new();
        }

        let distinct = distinct.clamp(1, total);
        let mut unique_keys: Vec<_> = (0..distinct).map(|_| UUID::gen_v4()).collect();
        unique_keys.sort_unstable();

        let mut keys = Vec::with_capacity(total);
        for idx in 0..total {
            keys.push(unique_keys[idx % unique_keys.len()]);
        }
        keys.sort_unstable();
        keys
    }

    fn delete_many_reference(
        tree: HtreeNode<u64>,
        delete_keys: &[UUID],
        store: &InMemoryStore,
    ) -> HtreeNode<u64> {
        let normalized_keys = unique_sorted(delete_keys.to_vec());
        tree.retain(|key, _| normalized_keys.binary_search(&key).is_err(), store)
            .expect("retain should succeed")
    }

    fn assert_matches_reference(
        tree: &HtreeNode<u64>,
        delete_keys: &[UUID],
        store: &InMemoryStore,
    ) {
        let actual = tree
            .delete_many(delete_keys.iter(), store)
            .expect("delete_many should succeed");
        let expected = delete_many_reference(tree.clone(), delete_keys, store);

        assert_eq!(
            collect_sorted_keys(&actual, store),
            collect_sorted_keys(&expected, store)
        );
    }

    #[test]
    fn delete_many_leaf_with_matching_key_returns_default() {
        let store = InMemoryStore::default();
        let key = UUID::gen_v4();

        let leaf = HtreeNode::from_kvp(&key, &7_u64, &store).expect("from_kvp should succeed");
        let deleted = leaf
            .delete_many([&key], &store)
            .expect("delete_many should succeed");

        assert!(deleted.is_empty());
    }

    #[test]
    fn delete_many_leaf_with_missing_key_is_noop() {
        let store = InMemoryStore::default();
        let key = UUID::gen_v4();
        let missing = UUID::gen_v4();

        let leaf = HtreeNode::from_kvp(&key, &11_u64, &store).expect("from_kvp should succeed");
        let deleted = leaf
            .delete_many([&missing], &store)
            .expect("delete_many should succeed");

        assert_eq!(collect_sorted_keys(&deleted, &store), vec![key]);
    }

    #[test]
    fn delete_many_empty_tree_is_noop() {
        let store = InMemoryStore::default();
        let empty: HtreeNode<u64> = HtreeNode::default();
        let delete_keys: Vec<_> = (0..8).map(|_| UUID::gen_v4()).collect();

        let deleted = empty
            .delete_many(delete_keys.iter(), &store)
            .expect("delete_many should succeed");

        assert!(deleted.is_empty());
    }

    #[test]
    fn delete_many_unsorted_duplicate_inputs_match_reference() {
        let store = InMemoryStore::default();
        let keys = patterned_keys(40, 24);
        let tree = tree_from_keys(&keys, &store);
        let unique = unique_sorted(keys);

        let delete_keys = vec![unique[7], unique[1], unique[7], unique[3], unique[1]];
        assert_matches_reference(&tree, &delete_keys, &store);
    }

    #[test]
    fn delete_many_removes_all_duplicates_across_sibling_subtrees() {
        let store = InMemoryStore::default();
        let duplicate_key = UUID::gen_v4();

        let leaves: Vec<_> = (0..=MAX_CHILDREN)
            .map(|idx| {
                HtreeNode::from_kvp(&duplicate_key, &(idx as u64), &store)
                    .expect("expected success")
            })
            .collect();

        let mut roots = HtreeNode::from_many_children(leaves, &store).expect("expected success");
        while roots.len() > 1 {
            roots = HtreeNode::from_many_children(roots, &store).expect("expected success");
        }

        let root = roots.into_iter().next().unwrap_or_default();
        let before_count = root.iter_keys(&store).count();
        assert_eq!(before_count, MAX_CHILDREN + 1);

        let deleted = root
            .delete_many([&duplicate_key], &store)
            .expect("expected success");

        assert!(
            !deleted
                .contains_key(&duplicate_key, &store)
                .expect("expected success")
        );
        assert_eq!(deleted.iter_keys(&store).count(), 0);
    }

    #[test]
    fn delete_many_removes_multiple_duplicate_runs_spanning_siblings() {
        let store = InMemoryStore::default();

        let mut ordered_keys = [UUID::gen_v4(), UUID::gen_v4(), UUID::gen_v4()];
        ordered_keys.sort_unstable();
        let key_a = ordered_keys[0];
        let key_b = ordered_keys[1];
        let key_c = ordered_keys[2];

        let mut keys = Vec::new();
        keys.extend(std::iter::repeat_n(key_a, MAX_CHILDREN + 1));
        keys.extend(std::iter::repeat_n(key_b, MAX_CHILDREN + 2));
        keys.extend(std::iter::repeat_n(key_c, 3));

        let tree = tree_from_keys(&keys, &store);
        let deleted = tree
            .delete_many([&key_a, &key_b], &store)
            .expect("delete_many should succeed");

        assert_eq!(key_occurrence_count(&deleted, key_a, &store), 0);
        assert_eq!(key_occurrence_count(&deleted, key_b, &store), 0);
        assert_eq!(key_occurrence_count(&deleted, key_c, &store), 3);
    }

    #[test]
    fn delete_many_is_idempotent_for_mixed_existing_and_missing_keys() {
        let store = InMemoryStore::default();
        let keys = patterned_keys(MAX_CHILDREN + 19, 17);
        let tree = tree_from_keys(&keys, &store);
        let unique = unique_sorted(keys);
        let missing = missing_keys(&unique, 6);

        let delete_keys = vec![
            unique[0],
            missing[0],
            unique[0],
            unique[unique.len() / 2],
            missing[3],
            unique[unique.len() - 1],
            missing[0],
        ];

        let once = tree
            .delete_many(delete_keys.iter(), &store)
            .expect("delete_many should succeed");
        let twice = once
            .delete_many(delete_keys.iter(), &store)
            .expect("delete_many should succeed");

        assert_eq!(
            collect_sorted_keys(&once, &store),
            collect_sorted_keys(&twice, &store)
        );
        assert_matches_reference(&tree, &delete_keys, &store);
    }

    #[test]
    fn delete_many_matches_retain_reference_for_varied_tree_sizes() {
        let store = InMemoryStore::default();
        let varied_sizes = [
            0,
            1,
            2,
            3,
            7,
            16,
            MAX_CHILDREN.saturating_sub(1),
            MAX_CHILDREN,
            MAX_CHILDREN + 1,
            MAX_CHILDREN + 17,
        ];

        for size in varied_sizes {
            let distinct = if size == 0 { 0 } else { (size / 3).max(1) };
            let keys = patterned_keys(size, distinct);
            let tree = tree_from_keys(&keys, &store);
            let unique = unique_sorted(keys);
            let missing = missing_keys(&unique, 4);

            let mut delete_cases = vec![Vec::new(), missing.clone()];
            if let Some(first) = unique.first() {
                delete_cases.push(vec![*first]);
            }
            if let Some(last) = unique.last() {
                delete_cases.push(vec![*last, *last]);
            }
            if unique.len() >= 3 {
                delete_cases.push(vec![unique[2], unique[0], unique[2], unique[1]]);
            }
            if !unique.is_empty() {
                delete_cases.push(unique.clone());
            }

            let mut mixed = missing;
            if let Some(first) = unique.first() {
                mixed.push(*first);
                mixed.push(*first);
            }
            if let Some(last) = unique.last() {
                mixed.push(*last);
            }
            mixed.reverse();
            delete_cases.push(mixed);

            for delete_keys in delete_cases {
                assert_matches_reference(&tree, &delete_keys, &store);
            }
        }
    }

    #[test]
    fn delete_many_matches_retain_reference_for_mixed_duplicate_patterns() {
        let store = InMemoryStore::default();

        for case_idx in 0_usize..32 {
            let total = 5 + case_idx;
            let distinct = (case_idx % 9) + 1;
            let mut keys = patterned_keys(total, distinct.min(total));
            if !keys.is_empty() {
                let key_count = keys.len();
                keys.rotate_left(case_idx % key_count);
            }

            let tree = tree_from_keys(&keys, &store);
            let unique = unique_sorted(keys);
            let missing = missing_keys(&unique, 8);

            let mut delete_keys = Vec::new();
            for idx in 0_usize..12 {
                if idx % 3 == 0 && !unique.is_empty() {
                    delete_keys.push(unique[(idx + case_idx) % unique.len()]);
                } else {
                    delete_keys.push(missing[(idx + case_idx) % missing.len()]);
                }

                if idx % 4 == 0 {
                    delete_keys.push(*delete_keys.last().expect("delete_keys should not be empty"));
                }
            }

            assert_matches_reference(&tree, &delete_keys, &store);
        }
    }
}
