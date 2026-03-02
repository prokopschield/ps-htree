use ps_hkey::Store;

use crate::HtreeNode;

impl<T> HtreeNode<T> {
    /// Combines multiple non-overlapping subtrees into a single tree.
    ///
    /// Accepts subtrees of any height. The subtrees must be *disjoint*: their
    /// key ranges must not overlap. The result contains all data from the
    /// input subtrees, preserving their internal structure.
    ///
    /// Empty subtrees are ignored. If all inputs are empty, returns a default
    /// (empty) node.
    ///
    /// # Errors
    ///
    /// Returns an error if store operations or tree reconstruction fail.
    pub fn merge_disjoint_subtrees<I, S>(
        subtrees: I,
        store: &S,
    ) -> Result<Self, HtreeNodeMergeDisjointSubtreesError<S>>
    where
        I: IntoIterator<Item = Self>,
        S: Store,
    {
        let mut nodes: Vec<Self> = subtrees.into_iter().filter(|n| !n.is_empty()).collect();

        if nodes.is_empty() {
            return Ok(Self::default());
        }

        if nodes.len() == 1 {
            return Ok(nodes.pop().unwrap_or_default());
        }

        // Sort by key to maintain ordering invariants
        nodes.sort();

        // Repeatedly lift contiguous runs of minimum-height nodes until uniform.
        // We must process in key order to avoid grouping non-adjacent ranges.
        loop {
            let min_height = nodes.iter().map(Self::height).min().unwrap_or(0);
            let max_height = nodes.iter().map(Self::height).max().unwrap_or(0);

            if min_height == max_height {
                // All nodes at same height - combine them all
                nodes = Self::from_many_children(nodes, store)?;

                // If only one node remains, return it
                if nodes.len() <= 1 {
                    return Ok(nodes.pop().unwrap_or_default());
                }

                // Continue looping to build more parent levels if needed
                continue;
            }

            // Lift contiguous runs of min-height nodes, preserving key order
            let mut result = Vec::with_capacity(nodes.len());
            let mut run: Vec<Self> = Vec::new();

            for node in nodes {
                if node.height() == min_height {
                    run.push(node);
                } else {
                    // Flush the current run before adding the taller node
                    if !run.is_empty() {
                        #[allow(clippy::iter_with_drain)]
                        result.extend(Self::from_many_children(run.drain(..), store)?);
                    }

                    result.push(node);
                }
            }

            // Flush any remaining run
            if !run.is_empty() {
                result.extend(Self::from_many_children(run, store)?);
            }

            nodes = result;
        }
    }
}

/// Errors that can occur when merging disjoint subtrees.
#[derive(thiserror::Error, Debug)]
pub enum HtreeNodeMergeDisjointSubtreesError<S: Store> {
    /// Node reconstruction failed.
    #[error("Node reconstruction failed.")]
    FromChildren(crate::HtreeNodeFromChildrenError<S>),

    /// A store operation failed.
    #[error("Store error: {0}")]
    Store(S::Error),
}

impl<S: Store> From<crate::HtreeNodeFromChildrenError<S>>
    for HtreeNodeMergeDisjointSubtreesError<S>
{
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
    use ps_hkey::InMemoryStore;
    use ps_uuid::UUID;

    use crate::HtreeNode;

    // ==================== Helper functions ====================

    fn make_leaf(key: &UUID, value: u64, store: &InMemoryStore) -> HtreeNode<u64> {
        HtreeNode::from_kvp(key, &value, store).expect("from_kvp should succeed")
    }

    fn sorted_keys(n: usize) -> Vec<UUID> {
        let mut keys: Vec<UUID> = (0..n).map(|_| UUID::gen_v4()).collect();
        keys.sort();
        keys
    }

    fn collect_keys(tree: &HtreeNode<u64>, store: &InMemoryStore) -> Vec<UUID> {
        tree.iter_keys(store)
            .collect::<Result<Vec<_>, _>>()
            .expect("iter_keys should succeed")
    }

    fn make_tree(keys: &[UUID], store: &InMemoryStore) -> HtreeNode<u64> {
        if keys.is_empty() {
            return HtreeNode::default();
        }

        let leaves: Vec<_> = keys
            .iter()
            .enumerate()
            .map(|(i, k)| make_leaf(k, i as u64, store))
            .collect();

        let parents = HtreeNode::from_many_children(leaves, store)
            .expect("from_many_children should succeed");

        if parents.len() <= 1 {
            return parents.into_iter().next().unwrap_or_default();
        }

        HtreeNode::from_children(parents, store).expect("from_children should succeed")
    }

    // ==================== Tests ====================

    #[test]
    fn test_empty_input() {
        let store = InMemoryStore::default();
        let result: HtreeNode<u64> =
            HtreeNode::merge_disjoint_subtrees([], &store).expect("should succeed");

        assert!(result.is_empty());
    }

    #[test]
    fn test_single_leaf() {
        let store = InMemoryStore::default();
        let key = UUID::gen_v4();
        let leaf = make_leaf(&key, 42, &store);

        let result =
            HtreeNode::merge_disjoint_subtrees([leaf.clone()], &store).expect("should succeed");

        assert_eq!(result.key, leaf.key);
        assert_eq!(result.height(), 0);
    }

    #[test]
    fn test_multiple_leaves_same_height() {
        let store = InMemoryStore::default();
        let keys = sorted_keys(5);

        let leaves: Vec<_> = keys
            .iter()
            .enumerate()
            .map(|(i, k)| make_leaf(k, i as u64, &store))
            .collect();

        let result = HtreeNode::merge_disjoint_subtrees(leaves, &store).expect("should succeed");

        let result_keys = collect_keys(&result, &store);
        assert_eq!(result_keys, keys);
    }

    #[test]
    fn test_subtrees_different_heights() {
        let store = InMemoryStore::default();

        // Create three DISJOINT key ranges using simple sequential prefixes
        // Only vary the first byte to ensure clear ordering
        let keys_a: Vec<UUID> = (0..1)
            .map(|i: u8| {
                let mut bytes = [0u8; 16];
                bytes[0] = 0x10; // Range A: 0x10...
                bytes[15] = i;
                UUID::from(bytes)
            })
            .collect();

        let keys_b: Vec<UUID> = (0..50)
            .map(|i: u8| {
                let mut bytes = [0u8; 16];
                bytes[0] = 0x20; // Range B: 0x20...
                bytes[15] = i;
                UUID::from(bytes)
            })
            .collect();

        let keys_c: Vec<UUID> = (0..10)
            .map(|i: u8| {
                let mut bytes = [0u8; 16];
                bytes[0] = 0x30; // Range C: 0x30...
                bytes[15] = i;
                UUID::from(bytes)
            })
            .collect();

        let mut all_keys: Vec<UUID> = keys_a
            .iter()
            .chain(keys_b.iter())
            .chain(keys_c.iter())
            .copied()
            .collect();
        all_keys.sort();

        let tree_a = make_tree(&keys_a, &store);
        let tree_b = make_tree(&keys_b, &store);
        let tree_c = make_tree(&keys_c, &store);

        let result = HtreeNode::merge_disjoint_subtrees([tree_a, tree_b, tree_c], &store)
            .expect("should succeed");

        let result_keys = collect_keys(&result, &store);
        assert_eq!(result_keys, all_keys);
    }

    #[test]
    fn test_with_empty_nodes() {
        let store = InMemoryStore::default();
        let keys = sorted_keys(3);

        let leaves: Vec<_> = keys
            .iter()
            .enumerate()
            .map(|(i, k)| make_leaf(k, i as u64, &store))
            .collect();

        let empty1: HtreeNode<u64> = HtreeNode::default();
        let empty2: HtreeNode<u64> = HtreeNode::default();

        let input: Vec<HtreeNode<u64>> = vec![
            empty1,
            leaves[0].clone(),
            empty2,
            leaves[1].clone(),
            leaves[2].clone(),
        ];

        let result = HtreeNode::merge_disjoint_subtrees(input, &store).expect("should succeed");

        let result_keys = collect_keys(&result, &store);
        assert_eq!(result_keys, keys);
    }

    #[test]
    fn test_preserves_key_ordering() {
        let store = InMemoryStore::default();

        // Create subtrees with specific key ranges (not pre-sorted in input)
        let keys_high: Vec<UUID> = (0..5)
            .map(|i| {
                let mut bytes = [0xFFu8; 16];
                bytes[15] = i;
                UUID::from(bytes)
            })
            .collect();

        let keys_low: Vec<UUID> = (0..5)
            .map(|i| {
                let mut bytes = [0x00u8; 16];
                bytes[15] = i;
                UUID::from(bytes)
            })
            .collect();

        let tree_high = make_tree(&keys_high, &store);
        let tree_low = make_tree(&keys_low, &store);

        // Input in "wrong" order (high before low)
        let result = HtreeNode::merge_disjoint_subtrees([tree_high, tree_low], &store)
            .expect("should succeed");

        let result_keys = collect_keys(&result, &store);

        // Result should be sorted: low keys first, then high keys
        let mut expected: Vec<UUID> = keys_low.iter().chain(keys_high.iter()).copied().collect();
        expected.sort();
        assert_eq!(result_keys, expected);
    }

    #[test]
    fn test_all_empty_nodes() {
        let store = InMemoryStore::default();

        let empties: Vec<HtreeNode<u64>> = (0..5).map(|_| HtreeNode::default()).collect();

        let result = HtreeNode::merge_disjoint_subtrees(empties, &store).expect("should succeed");

        assert!(result.is_empty());
    }
}
