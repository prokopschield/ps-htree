use ps_hkey::Store;
use ps_uuid::UUID;

use crate::HtreeNode;

impl<T> HtreeNode<T> {
    /// Splits the tree into multiple partitions at the given keys.
    ///
    /// Given `n` split keys `[k0, k1, ..., kn-1]`, returns `n + 1` partitions:
    /// - Partition 0: `[..k0)` — leaves with key < k0
    /// - Partition i: `[ki-1..ki)` — leaves with ki-1 <= key < ki
    /// - Partition n: `[kn-1..)` — leaves with key >= kn-1
    ///
    /// Empty partitions are returned as `None`.
    ///
    /// Performs a single recursive traversal, routing split keys to child
    /// ranges and rebuilding output partitions once at the end.
    ///
    /// # Arguments
    ///
    /// * `keys` - Split keys in strictly ascending order. Duplicate keys are
    ///   allowed but will produce empty partitions between them.
    /// * `store` - Persistence backend.
    ///
    /// # Panics
    ///
    /// Debug builds panic if `keys` is not sorted.
    ///
    /// # Errors
    ///
    /// Returns [`HtreeNodeSplitManyError`] on store failures or corrupted node state.
    pub fn split_many<S>(
        &self,
        keys: &[UUID],
        store: &S,
    ) -> Result<Vec<Option<Self>>, HtreeNodeSplitManyError<S>>
    where
        S: Store,
    {
        debug_assert!(
            keys.windows(2).all(|w| w[0] <= w[1]),
            "split_many: keys must be sorted"
        );

        let n = keys.len() + 1;

        if keys.is_empty() {
            return Ok(vec![if self.is_empty() {
                None
            } else {
                Some(self.clone())
            }]);
        }

        if self.is_empty() {
            return Ok(vec![None; n]);
        }

        self.split_many_collect(keys, store)?
            .into_iter()
            .map(|nodes| rebuild_tree(nodes, store))
            .collect()
    }

    /// Internal single-traversal splitter.
    ///
    /// Returns `keys.len() + 1` forests where each forest is already key-sorted
    /// but may contain mixed heights.
    fn split_many_collect<S: Store>(
        &self,
        keys: &[UUID],
        store: &S,
    ) -> Result<Vec<Vec<Self>>, HtreeNodeSplitManyError<S>> {
        let n = keys.len() + 1;

        if self.is_empty() {
            return Ok(vec![Vec::new(); n]);
        }

        if self.is_leaf() {
            let idx = keys.partition_point(|k| k <= &self.key);
            let mut result = vec![Vec::new(); n];
            result[idx].push(self.clone());
            return Ok(result);
        }

        let children = self.fetch_children(store)?;
        let mut partitions: Vec<Vec<Self>> = vec![Vec::new(); n];

        let Some(first_child) = children.first() else {
            return Ok(partitions);
        };

        // Skip keys strictly smaller than the first child key: they map to
        // leading empty partitions.
        let mut key_cursor = keys.partition_point(|k| k < &first_child.key);
        let mut base_partition = key_cursor;

        for (i, child) in children.iter().enumerate() {
            // Child i owns keys in [child.key, next_child.key), or [child.key, +inf)
            // for the last child.
            let start = key_cursor;
            if let Some(next_child) = children.get(i + 1) {
                while key_cursor < keys.len() && keys[key_cursor] < next_child.key {
                    key_cursor += 1;
                }
            } else {
                key_cursor = keys.len();
            }

            let child_keys = &keys[start..key_cursor];

            if child_keys.is_empty() {
                partitions[base_partition].push(child.clone());
            } else {
                let child_parts = child.split_many_collect(child_keys, store)?;

                for (j, mut nodes) in child_parts.into_iter().enumerate() {
                    partitions[base_partition + j].append(&mut nodes);
                }
            }

            base_partition = key_cursor;
        }

        Ok(partitions)
    }
}

/// Rebuilds a tree from nodes that may have different heights.
///
/// Iteratively combines nodes from lowest to highest height until a single
/// tree remains.
fn rebuild_tree<T, S: Store>(
    mut nodes: Vec<HtreeNode<T>>,
    store: &S,
) -> Result<Option<HtreeNode<T>>, HtreeNodeSplitManyError<S>> {
    if nodes.is_empty() {
        return Ok(None);
    }

    if nodes.len() == 1 {
        return Ok(nodes.into_iter().next());
    }

    // Keep global key order stable while lifting lower-height segments.
    nodes.sort();

    while nodes.len() > 1 {
        let min_height = nodes
            .iter()
            .map(|node| node.height)
            .min()
            .ok_or(HtreeNodeSplitManyError::CorruptedState)?;

        let mut next = Vec::with_capacity(nodes.len());
        let mut i = 0;

        while i < nodes.len() {
            if nodes[i].height != min_height {
                next.push(nodes[i].clone());
                i += 1;
                continue;
            }

            // Combine one contiguous run of minimum-height nodes.
            let start = i;
            i += 1;
            while i < nodes.len() && nodes[i].height == min_height {
                i += 1;
            }

            let combined = HtreeNode::from_children(nodes[start..i].iter().cloned(), store)?;
            next.push(combined);
        }

        nodes = next;
    }

    Ok(nodes.into_iter().next())
}

#[derive(thiserror::Error, Debug)]
pub enum HtreeNodeSplitManyError<S: Store> {
    #[error("HtreeNode's state is corrupted.")]
    CorruptedState,

    #[error("Node reconstruction failed.")]
    FromChildren(crate::HtreeNodeFromChildrenError<S>),

    #[error("Store error: {0}")]
    Store(S::Error),

    #[error("Error unpacking children: {0}")]
    UnpackChildren(#[from] crate::HtreeNodeUnpackChildrenError),
}

impl<S: Store> From<crate::HtreeNodeFetchChildrenError<S>> for HtreeNodeSplitManyError<S> {
    fn from(value: crate::HtreeNodeFetchChildrenError<S>) -> Self {
        match value {
            crate::HtreeNodeFetchChildrenError::CorruptedState => Self::CorruptedState,
            crate::HtreeNodeFetchChildrenError::Store(err) => Self::Store(err),
            crate::HtreeNodeFetchChildrenError::UnpackChildren(err) => Self::UnpackChildren(err),
        }
    }
}

impl<S: Store> From<crate::HtreeNodeFromChildrenError<S>> for HtreeNodeSplitManyError<S> {
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

    fn collect_keys(
        tree: &HtreeNode<u64>,
        store: &InMemoryStore,
    ) -> Result<Vec<UUID>, Box<dyn std::error::Error>> {
        Ok(tree.iter_keys(store).collect::<Result<Vec<_>, _>>()?)
    }

    fn make_tree(
        keys: &[UUID],
        store: &InMemoryStore,
    ) -> Result<HtreeNode<u64>, Box<dyn std::error::Error>> {
        if keys.is_empty() {
            return Ok(HtreeNode::default());
        }

        let leaves = keys
            .iter()
            .enumerate()
            .map(|(i, k)| HtreeNode::<u64>::from_kvp(k, &(i as u64), store))
            .collect::<Result<Vec<_>, _>>()?;

        let mut nodes = HtreeNode::from_many_children(leaves, store)?;
        while nodes.len() > 1 {
            nodes = HtreeNode::from_many_children(nodes, store)?;
        }
        nodes
            .into_iter()
            .next()
            .ok_or_else(|| "expected at least one node".into())
    }

    fn gen_keys(n: usize) -> Vec<UUID> {
        let mut keys: Vec<UUID> = (0..n).map(|_| UUID::gen_v4()).collect();
        keys.sort();
        keys
    }

    fn assert_partition_boundaries(
        parts: &[Option<HtreeNode<u64>>],
        split_keys: &[UUID],
        store: &InMemoryStore,
    ) -> Result<(), Box<dyn std::error::Error>> {
        for (i, part) in parts.iter().enumerate() {
            if let Some(tree) = part {
                let keys = collect_keys(tree, store)?;
                for key in &keys {
                    // Key should be >= split_keys[i-1] (if exists)
                    if i > 0 {
                        assert!(
                            key >= &split_keys[i - 1],
                            "Partition {i}: key {key:?} < lower bound {:?}",
                            split_keys[i - 1]
                        );
                    }
                    // Key should be < split_keys[i] (if exists)
                    if i < split_keys.len() {
                        assert!(
                            key < &split_keys[i],
                            "Partition {i}: key {key:?} >= upper bound {:?}",
                            split_keys[i]
                        );
                    }
                }
            }
        }
        Ok(())
    }

    // ==================== Basic functionality ====================

    #[test]
    fn split_many_empty_keys() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(10);
        let tree = make_tree(&keys, &store)?;

        let parts = tree.split_many(&[], &store)?;

        assert_eq!(parts.len(), 1);
        let part = parts[0].as_ref().ok_or("expected part")?;
        assert_eq!(collect_keys(part, &store)?, keys);
        Ok(())
    }

    #[test]
    fn split_many_empty_tree() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let tree: HtreeNode<u64> = HtreeNode::default();
        let split_keys = gen_keys(3);

        let parts = tree.split_many(&split_keys, &store)?;

        assert_eq!(parts.len(), 4);
        assert!(parts.iter().all(std::option::Option::is_none));
        Ok(())
    }

    #[test]
    fn split_many_single_key() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(10);
        let tree = make_tree(&keys, &store)?;

        let parts = tree.split_many(&[keys[5]], &store)?;

        assert_eq!(parts.len(), 2);

        let left = parts[0].as_ref().ok_or("expected left")?;
        let right = parts[1].as_ref().ok_or("expected right")?;

        let left_keys = collect_keys(left, &store)?;
        let right_keys = collect_keys(right, &store)?;

        assert_eq!(left_keys, &keys[..5]);
        assert_eq!(right_keys, &keys[5..]);
        Ok(())
    }

    #[test]
    fn split_many_multiple_keys() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(20);
        let tree = make_tree(&keys, &store)?;

        let split_keys = vec![keys[5], keys[10], keys[15]];
        let parts = tree.split_many(&split_keys, &store)?;

        assert_eq!(parts.len(), 4);

        let p0 = parts[0].as_ref().ok_or("expected p0")?;
        let p1 = parts[1].as_ref().ok_or("expected p1")?;
        let p2 = parts[2].as_ref().ok_or("expected p2")?;
        let p3 = parts[3].as_ref().ok_or("expected p3")?;

        assert_eq!(collect_keys(p0, &store)?, &keys[..5]);
        assert_eq!(collect_keys(p1, &store)?, &keys[5..10]);
        assert_eq!(collect_keys(p2, &store)?, &keys[10..15]);
        assert_eq!(collect_keys(p3, &store)?, &keys[15..]);
        Ok(())
    }

    #[test]
    fn split_many_preserves_all_keys() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(50);
        let tree = make_tree(&keys, &store)?;

        let split_keys: Vec<UUID> = keys.iter().step_by(10).copied().collect();
        let parts = tree.split_many(&split_keys, &store)?;

        let mut all_keys = Vec::new();
        for part in parts.into_iter().flatten() {
            all_keys.extend(collect_keys(&part, &store)?);
        }
        all_keys.sort();

        assert_eq!(all_keys, keys);
        Ok(())
    }

    #[test]
    fn split_many_consistent_with_sequential_splits() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(30);
        let tree = make_tree(&keys, &store)?;

        let split_keys = vec![keys[10], keys[20]];

        // Multi-way split
        let multi_parts = tree.split_many(&split_keys, &store)?;

        // Sequential splits
        let (p0, rest) = tree.split_at(&keys[10], &store)?;
        let (p1, p2) = rest
            .map(|r| r.split_at(&keys[20], &store))
            .transpose()?
            .unwrap_or((None, None));

        // Compare results
        let multi_keys: Vec<Vec<UUID>> = multi_parts
            .iter()
            .map(|p| {
                p.as_ref()
                    .map(|t| collect_keys(t, &store))
                    .transpose()
                    .map(std::option::Option::unwrap_or_default)
            })
            .collect::<Result<_, _>>()?;

        let seq_keys: Vec<Vec<UUID>> = [p0, p1, p2]
            .iter()
            .map(|p| {
                p.as_ref()
                    .map(|t| collect_keys(t, &store))
                    .transpose()
                    .map(std::option::Option::unwrap_or_default)
            })
            .collect::<Result<_, _>>()?;

        assert_eq!(multi_keys, seq_keys);
        Ok(())
    }

    #[test]
    fn split_many_with_gaps() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(20);
        let tree = make_tree(&keys, &store)?;

        // Split keys that create empty partitions
        let mut split_keys = gen_keys(2);
        // Make sure split_keys[0] < keys[0] and split_keys[1] > keys[last]
        split_keys[0] = UUID::nil();
        split_keys[1] = UUID::max();

        let parts = tree.split_many(&split_keys, &store)?;

        assert_eq!(parts.len(), 3);
        assert!(parts[0].is_none()); // Nothing before nil
        let middle = parts[1].as_ref().ok_or("expected middle")?;
        assert_eq!(collect_keys(middle, &store)?, keys);
        assert!(parts[2].is_none()); // Nothing after max
        Ok(())
    }

    #[test]
    fn split_many_large_tree() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(200);
        let tree = make_tree(&keys, &store)?;

        // Split into 10 parts
        let split_keys: Vec<UUID> = (1..10).map(|i| keys[i * 20]).collect();
        let parts = tree.split_many(&split_keys, &store)?;

        assert_eq!(parts.len(), 10);

        let mut all_keys = Vec::new();
        for part in parts.into_iter().flatten() {
            all_keys.extend(collect_keys(&part, &store)?);
        }
        all_keys.sort();

        assert_eq!(all_keys, keys);
        Ok(())
    }

    // ==================== Edge cases: single leaf ====================

    #[test]
    fn split_single_leaf_before_key() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let leaf_key = UUID::gen_v4();
        let tree = HtreeNode::<u64>::from_kvp(&leaf_key, &42, &store)?;

        // Split key greater than leaf
        let split_key = UUID::max();
        let parts = tree.split_many(&[split_key], &store)?;

        assert_eq!(parts.len(), 2);
        assert!(parts[0].is_some()); // Leaf goes to left partition
        assert!(parts[1].is_none());
        Ok(())
    }

    #[test]
    fn split_single_leaf_after_key() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let leaf_key = UUID::gen_v4();
        let tree = HtreeNode::<u64>::from_kvp(&leaf_key, &42, &store)?;

        // Split key less than leaf
        let split_key = UUID::nil();
        let parts = tree.split_many(&[split_key], &store)?;

        assert_eq!(parts.len(), 2);
        assert!(parts[0].is_none());
        assert!(parts[1].is_some()); // Leaf goes to right partition
        Ok(())
    }

    #[test]
    fn split_single_leaf_at_exact_key() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let leaf_key = UUID::gen_v4();
        let tree = HtreeNode::<u64>::from_kvp(&leaf_key, &42, &store)?;

        // Split at exactly the leaf's key
        let parts = tree.split_many(&[leaf_key], &store)?;

        assert_eq!(parts.len(), 2);
        assert!(parts[0].is_none()); // Nothing strictly less than key
        assert!(parts[1].is_some()); // Leaf goes to >= partition
        Ok(())
    }

    // ==================== Edge cases: boundary keys ====================

    #[test]
    fn split_at_first_key() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(10);
        let tree = make_tree(&keys, &store)?;

        let parts = tree.split_many(&[keys[0]], &store)?;

        assert_eq!(parts.len(), 2);
        assert!(parts[0].is_none()); // Nothing < first key
        let right = parts[1].as_ref().ok_or("expected right")?;
        assert_eq!(collect_keys(right, &store)?, keys); // All keys >= first
        Ok(())
    }

    #[test]
    fn split_at_last_key() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(10);
        let tree = make_tree(&keys, &store)?;

        let parts = tree.split_many(&[keys[9]], &store)?;

        assert_eq!(parts.len(), 2);
        let left = parts[0].as_ref().ok_or("expected left")?;
        assert_eq!(collect_keys(left, &store)?, &keys[..9]);
        let right = parts[1].as_ref().ok_or("expected right")?;
        assert_eq!(collect_keys(right, &store)?, &keys[9..]);
        Ok(())
    }

    #[test]
    fn split_all_keys_before_tree() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(10);
        let tree = make_tree(&keys, &store)?;

        // All split keys are less than tree's minimum
        let split_keys = vec![UUID::nil()];
        let parts = tree.split_many(&split_keys, &store)?;

        assert_eq!(parts.len(), 2);
        assert!(parts[0].is_none());
        let right = parts[1].as_ref().ok_or("expected right")?;
        assert_eq!(collect_keys(right, &store)?, keys);
        Ok(())
    }

    #[test]
    fn split_all_keys_after_tree() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(10);
        let tree = make_tree(&keys, &store)?;

        // All split keys are greater than tree's maximum
        let split_keys = vec![UUID::max()];
        let parts = tree.split_many(&split_keys, &store)?;

        assert_eq!(parts.len(), 2);
        let left = parts[0].as_ref().ok_or("expected left")?;
        assert_eq!(collect_keys(left, &store)?, keys);
        assert!(parts[1].is_none());
        Ok(())
    }

    // ==================== Edge cases: duplicate split keys ====================

    #[test]
    fn split_duplicate_keys_creates_empty_partitions() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(10);
        let tree = make_tree(&keys, &store)?;

        // Duplicate split keys
        let split_keys = vec![keys[5], keys[5], keys[5]];
        let parts = tree.split_many(&split_keys, &store)?;

        assert_eq!(parts.len(), 4);
        // Partitions 1 and 2 should be empty (between identical keys)
        let left = parts[0].as_ref().ok_or("expected left")?;
        assert_eq!(collect_keys(left, &store)?, &keys[..5]);
        assert!(parts[1].is_none()); // Empty: [keys[5], keys[5])
        assert!(parts[2].is_none()); // Empty: [keys[5], keys[5])
        let right = parts[3].as_ref().ok_or("expected right")?;
        assert_eq!(collect_keys(right, &store)?, &keys[5..]);
        Ok(())
    }

    // ==================== Edge cases: split at every key ====================

    #[test]
    fn split_at_every_key() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(20);
        let tree = make_tree(&keys, &store)?;

        // Split at every key in the tree
        let parts = tree.split_many(&keys, &store)?;

        assert_eq!(parts.len(), keys.len() + 1);

        // First partition should be empty (nothing < first key)
        assert!(parts[0].is_none());

        // Each subsequent partition should have exactly one key
        for (i, part) in parts.iter().enumerate().skip(1) {
            let tree = part
                .as_ref()
                .ok_or_else(|| format!("expected partition {i}"))?;
            let part_keys = collect_keys(tree, &store)?;
            assert_eq!(part_keys.len(), 1);
            assert_eq!(part_keys[0], keys[i - 1]);
        }
        Ok(())
    }

    // ==================== Partition boundary verification ====================

    #[test]
    fn partitions_respect_boundaries() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(100);
        let tree = make_tree(&keys, &store)?;

        let split_keys: Vec<UUID> = keys.iter().step_by(10).copied().collect();
        let parts = tree.split_many(&split_keys, &store)?;

        assert_partition_boundaries(&parts, &split_keys, &store)?;
        Ok(())
    }

    // ==================== Large scale stress tests ====================

    #[test]
    fn stress_10k_keys_100_splits() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(10_000);
        let tree = make_tree(&keys, &store)?;

        // Split into 101 partitions
        let split_keys: Vec<UUID> = (1..=100).map(|i| keys[i * 99]).collect();
        let parts = tree.split_many(&split_keys, &store)?;

        assert_eq!(parts.len(), 101);

        // Verify all keys preserved
        let mut all_keys = Vec::new();
        for part in parts.iter().flatten() {
            all_keys.extend(collect_keys(part, &store)?);
        }
        all_keys.sort();
        assert_eq!(all_keys, keys);

        Ok(())
    }

    #[test]
    fn stress_1k_keys_many_splits() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(1_000);
        let tree = make_tree(&keys, &store)?;

        // Split at every 5th key = 200 splits
        let split_keys: Vec<UUID> = keys.iter().step_by(5).copied().collect();
        let parts = tree.split_many(&split_keys, &store)?;

        assert_eq!(parts.len(), split_keys.len() + 1);
        assert_partition_boundaries(&parts, &split_keys, &store)?;

        // Verify all keys preserved
        let mut all_keys = Vec::new();
        for part in parts.iter().flatten() {
            all_keys.extend(collect_keys(part, &store)?);
        }
        all_keys.sort();
        assert_eq!(all_keys, keys);

        Ok(())
    }

    #[test]
    fn stress_consistent_with_sequential_large() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(500);
        let tree = make_tree(&keys, &store)?;

        let split_keys = vec![keys[100], keys[200], keys[300], keys[400]];

        // Multi-way split
        let multi_parts = tree.split_many(&split_keys, &store)?;

        // Sequential splits
        let (p0, rest0) = tree.split_at(&split_keys[0], &store)?;
        let (p1, rest1) = rest0
            .map(|r| r.split_at(&split_keys[1], &store))
            .transpose()?
            .unwrap_or((None, None));
        let (p2, rest2) = rest1
            .map(|r| r.split_at(&split_keys[2], &store))
            .transpose()?
            .unwrap_or((None, None));
        let (p3, p4) = rest2
            .map(|r| r.split_at(&split_keys[3], &store))
            .transpose()?
            .unwrap_or((None, None));

        // Compare key sets
        let multi_keys: Vec<Vec<UUID>> = multi_parts
            .iter()
            .map(|p| {
                p.as_ref()
                    .map(|t| collect_keys(t, &store))
                    .transpose()
                    .map(std::option::Option::unwrap_or_default)
            })
            .collect::<Result<_, _>>()?;

        let seq_keys: Vec<Vec<UUID>> = [p0, p1, p2, p3, p4]
            .iter()
            .map(|p| {
                p.as_ref()
                    .map(|t| collect_keys(t, &store))
                    .transpose()
                    .map(std::option::Option::unwrap_or_default)
            })
            .collect::<Result<_, _>>()?;

        assert_eq!(multi_keys, seq_keys);
        Ok(())
    }

    // ==================== Non-existent split keys ====================

    #[test]
    fn split_at_nonexistent_keys() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(20);
        let tree = make_tree(&keys, &store)?;

        // Generate split keys that don't exist in tree
        let mut split_keys = gen_keys(3);
        split_keys.sort();

        let parts = tree.split_many(&split_keys, &store)?;

        assert_eq!(parts.len(), 4);
        assert_partition_boundaries(&parts, &split_keys, &store)?;

        // Verify all keys preserved
        let mut all_keys = Vec::new();
        for part in parts.iter().flatten() {
            all_keys.extend(collect_keys(part, &store)?);
        }
        all_keys.sort();
        assert_eq!(all_keys, keys);
        Ok(())
    }

    // ==================== Does not mutate original ====================

    #[test]
    fn split_does_not_mutate_original() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(50);
        let tree = make_tree(&keys, &store)?;

        let original_keys = collect_keys(&tree, &store)?;

        let split_keys: Vec<UUID> = keys.iter().step_by(10).copied().collect();
        let _parts = tree.split_many(&split_keys, &store)?;

        // Original tree should be unchanged
        let after_keys = collect_keys(&tree, &store)?;
        assert_eq!(original_keys, after_keys);
        Ok(())
    }

    // ==================== Subtree preservation ====================
    // These tests verify that subtrees are NOT unnecessarily traversed.
    // If a subtree doesn't need splitting, its hkey should remain unchanged,
    // proving fetch_children() was not called on it.

    #[test]
    fn subtree_preserved_when_no_split_keys_in_range() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();

        // Create two subtrees with the same structure (same height)
        let keys1: Vec<UUID> = (0..30)
            .map(|i| {
                let mut bytes = [0u8; 16];
                bytes[0] = 0; // Low range
                bytes[15] = i;
                UUID::from_bytes(bytes)
            })
            .collect();

        let keys2: Vec<UUID> = (0..30)
            .map(|i| {
                let mut bytes = [0xFFu8; 16];
                bytes[15] = i;
                UUID::from_bytes(bytes)
            })
            .collect();

        // Build separate subtrees of same height
        let subtree1_leaves: Vec<HtreeNode<u64>> = keys1
            .iter()
            .enumerate()
            .map(|(i, k)| HtreeNode::<u64>::from_kvp(k, &(i as u64), &store))
            .collect::<Result<_, _>>()?;
        let subtree1 = HtreeNode::from_sorted_children(subtree1_leaves, &store)?;

        let subtree2_leaves: Vec<HtreeNode<u64>> = keys2
            .iter()
            .enumerate()
            .map(|(i, k)| HtreeNode::<u64>::from_kvp(k, &(i as u64), &store))
            .collect::<Result<_, _>>()?;
        let subtree2 = HtreeNode::from_sorted_children(subtree2_leaves, &store)?;

        // Both subtrees have the same height
        assert_eq!(subtree1.height, subtree2.height);

        let tree = HtreeNode::from_sorted_children(vec![subtree1, subtree2], &store)?;

        // Split between the two subtrees - neither subtree needs to be traversed
        let split_key = {
            let bytes = [0x80u8; 16];
            UUID::from_bytes(bytes)
        };
        let parts = tree.split_many(&[split_key], &store)?;

        // The left partition should contain subtree1's keys
        let left = parts[0].as_ref().ok_or("expected left partition")?;
        let right = parts[1].as_ref().ok_or("expected right partition")?;

        let left_keys = collect_keys(left, &store)?;
        let right_keys = collect_keys(right, &store)?;

        assert_eq!(left_keys, keys1);
        assert_eq!(right_keys, keys2);

        Ok(())
    }

    #[test]
    fn single_subtree_preserved_when_entirely_in_one_partition()
    -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();

        // Create a parent with multiple children
        let mut all_keys = Vec::new();
        let mut children = Vec::new();

        for batch in 0..5 {
            let batch_keys: Vec<UUID> = (0..20)
                .map(|i| {
                    let mut bytes = [0u8; 16];
                    bytes[0] = batch;
                    bytes[15] = i;
                    UUID::from_bytes(bytes)
                })
                .collect();
            all_keys.extend(batch_keys.clone());

            let leaves: Vec<HtreeNode<u64>> = batch_keys
                .iter()
                .enumerate()
                .map(|(i, k)| HtreeNode::<u64>::from_kvp(k, &(i as u64), &store))
                .collect::<Result<_, _>>()?;
            let child = HtreeNode::from_sorted_children(leaves, &store)?;
            children.push(child);
        }

        all_keys.sort();
        children.sort();
        let tree = HtreeNode::from_sorted_children(children.clone(), &store)?;

        // Note: We don't compare hkeys because rebuild_tree creates new parents.
        // The key preservation tests verify the behavior we care about.

        // Split between batch 2 and batch 3 - batches 0,1,2 go left, 3,4 go right
        let split_key = {
            let mut bytes = [0u8; 16];
            bytes[0] = 3;
            UUID::from_bytes(bytes)
        };

        let parts = tree.split_many(&[split_key], &store)?;

        let left = parts[0].as_ref().ok_or("expected left")?;
        let right = parts[1].as_ref().ok_or("expected right")?;

        // Verify keys are correctly partitioned
        let left_keys = collect_keys(left, &store)?;
        let right_keys = collect_keys(right, &store)?;

        let expected_left: Vec<_> = all_keys
            .iter()
            .filter(|k| *k < &split_key)
            .copied()
            .collect();
        let expected_right: Vec<_> = all_keys
            .iter()
            .filter(|k| *k >= &split_key)
            .copied()
            .collect();

        assert_eq!(left_keys, expected_left);
        assert_eq!(right_keys, expected_right);

        Ok(())
    }

    #[test]
    fn leaf_hkey_preserved_when_not_split() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();

        let key1 = UUID::nil();
        let key2 = UUID::max();

        let leaf1 = HtreeNode::<u64>::from_kvp(&key1, &1, &store)?;
        let leaf2 = HtreeNode::<u64>::from_kvp(&key2, &2, &store)?;

        let leaf1_hkey = leaf1.hkey.clone();
        let leaf2_hkey = leaf2.hkey.clone();

        let tree = HtreeNode::from_sorted_children(vec![leaf1, leaf2], &store)?;

        // Split between the two leaves
        let split_key_bytes = [0x80u8; 16]; // Middle value
        let split_key = UUID::from_bytes(split_key_bytes);

        let parts = tree.split_many(&[split_key], &store)?;

        let left = parts[0].as_ref().ok_or("expected left")?;
        let right = parts[1].as_ref().ok_or("expected right")?;

        // Each partition should contain exactly one leaf
        // The leaf's hkey should be unchanged (proving it wasn't reconstructed)
        assert!(left.is_leaf());
        assert!(right.is_leaf());
        assert_eq!(left.hkey, leaf1_hkey);
        assert_eq!(right.hkey, leaf2_hkey);

        Ok(())
    }

    #[test]
    fn multiple_unsplit_subtrees_preserved() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();

        // Create 10 subtrees, each with 10 leaves
        let mut children = Vec::new();

        for batch in 0..10u8 {
            let batch_keys: Vec<UUID> = (0..10)
                .map(|i| {
                    let mut bytes = [0u8; 16];
                    bytes[0] = batch;
                    bytes[15] = i;
                    UUID::from_bytes(bytes)
                })
                .collect();

            let leaves: Vec<HtreeNode<u64>> = batch_keys
                .iter()
                .enumerate()
                .map(|(i, k)| HtreeNode::<u64>::from_kvp(k, &(i as u64), &store))
                .collect::<Result<_, _>>()?;

            let child = HtreeNode::from_sorted_children(leaves, &store)?;
            children.push(child);
        }

        let tree = HtreeNode::from_sorted_children(children, &store)?;

        // Split only between subtrees 4 and 5 - no subtree should be traversed
        let split_key = {
            let mut bytes = [0u8; 16];
            bytes[0] = 5;
            UUID::from_bytes(bytes)
        };

        let parts = tree.split_many(&[split_key], &store)?;

        // Verify keys are correctly split
        let left = parts[0].as_ref().ok_or("expected left")?;
        let right = parts[1].as_ref().ok_or("expected right")?;

        let left_keys = collect_keys(left, &store)?;
        let right_keys = collect_keys(right, &store)?;

        assert_eq!(left_keys.len(), 50); // 5 subtrees * 10 leaves
        assert_eq!(right_keys.len(), 50);

        Ok(())
    }

    // ==================== Height handling in rebuild_tree ====================

    #[test]
    fn rebuild_handles_mixed_heights() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();

        // Create a tree where splitting produces nodes of different heights
        let mut keys = Vec::new();

        // Group 1: many leaves that will form a subtree
        for i in 0..50 {
            let mut bytes = [0u8; 16];
            bytes[0] = 0;
            bytes[15] = i;
            keys.push(UUID::from_bytes(bytes));
        }

        // Group 2: few leaves (will remain as leaves or shallow tree)
        for i in 0..3 {
            let mut bytes = [0u8; 16];
            bytes[0] = 2;
            bytes[15] = i;
            keys.push(UUID::from_bytes(bytes));
        }

        keys.sort();
        let tree = make_tree(&keys, &store)?;

        // Split in the middle - this should produce partitions with different heights
        let split_key = {
            let mut bytes = [0u8; 16];
            bytes[0] = 1;
            UUID::from_bytes(bytes)
        };

        let parts = tree.split_many(&[split_key], &store)?;

        // Both partitions should be valid trees
        let left = parts[0].as_ref().ok_or("expected left")?;
        let right = parts[1].as_ref().ok_or("expected right")?;

        let left_keys = collect_keys(left, &store)?;
        let right_keys = collect_keys(right, &store)?;

        assert_eq!(left_keys.len(), 50);
        assert_eq!(right_keys.len(), 3);

        // Verify all keys preserved
        let mut all_recovered: Vec<_> = left_keys.into_iter().chain(right_keys).collect();
        all_recovered.sort();
        assert_eq!(all_recovered, keys);

        Ok(())
    }

    #[test]
    fn rebuild_single_node_returns_it() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let key = UUID::gen_v4();
        let tree = HtreeNode::<u64>::from_kvp(&key, &42, &store)?;

        // Split at a key that puts the single leaf in one partition
        let parts = tree.split_many(&[UUID::nil()], &store)?;

        assert!(parts[0].is_none());
        let right = parts[1].as_ref().ok_or("expected right")?;
        assert!(right.is_leaf());
        assert_eq!(right.key, key);

        Ok(())
    }

    // ==================== Deep tree tests ====================

    #[test]
    fn split_deep_tree() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();

        // Create a very deep tree by using sequential keys
        let keys = gen_keys(500);
        let tree = make_tree(&keys, &store)?;

        // Tree should have some height
        assert!(
            tree.height > 1,
            "Expected deep tree, got height {}",
            tree.height
        );

        // Split into many partitions
        let split_keys: Vec<UUID> = keys.iter().step_by(25).copied().collect();
        let parts = tree.split_many(&split_keys, &store)?;

        // Verify all keys preserved
        let mut all_keys = Vec::new();
        for part in parts.iter().flatten() {
            all_keys.extend(collect_keys(part, &store)?);
        }
        all_keys.sort();
        assert_eq!(all_keys, keys);

        Ok(())
    }

    #[test]
    fn split_tall_tree_at_internal_boundaries() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(200);
        let tree = make_tree(&keys, &store)?;

        // Get the tree's direct children to find internal boundaries
        let children = tree.fetch_children(&store)?;
        assert!(
            children.len() >= 2,
            "Need at least 2 children for this test"
        );

        // Split exactly at a child boundary (first child's max key)
        let split_key = children[1].key;
        let parts = tree.split_many(&[split_key], &store)?;

        // Verify partitioning
        let left_keys = parts[0]
            .as_ref()
            .map(|t| collect_keys(t, &store))
            .transpose()?
            .unwrap_or_default();
        let right_keys = parts[1]
            .as_ref()
            .map(|t| collect_keys(t, &store))
            .transpose()?
            .unwrap_or_default();

        for key in &left_keys {
            assert!(key < &split_key);
        }
        for key in &right_keys {
            assert!(key >= &split_key);
        }

        Ok(())
    }

    // ==================== Many split keys ====================

    #[test]
    fn many_consecutive_split_keys() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(100);
        let tree = make_tree(&keys, &store)?;

        // Create many split keys between keys[50] and keys[51]
        let base = keys[50];
        let split_keys = vec![base; 10]; // 10 duplicate split keys

        let parts = tree.split_many(&split_keys, &store)?;

        assert_eq!(parts.len(), 11);

        // First partition has keys < base
        let first_keys = parts[0]
            .as_ref()
            .map(|t| collect_keys(t, &store))
            .transpose()?
            .unwrap_or_default();
        for key in &first_keys {
            assert!(key < &base);
        }

        // Middle partitions (1-9) should be empty (between duplicate keys)
        for (i, part) in parts.iter().take(10).enumerate().skip(1) {
            assert!(
                part.is_none(),
                "Partition {i} should be empty between duplicates"
            );
        }

        // Last partition has keys >= base
        let last_keys = parts[10]
            .as_ref()
            .map(|t| collect_keys(t, &store))
            .transpose()?
            .unwrap_or_default();
        for key in &last_keys {
            assert!(key >= &base);
        }

        Ok(())
    }

    #[test]
    fn split_with_more_keys_than_leaves() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(5);
        let tree = make_tree(&keys, &store)?;

        // 20 split keys for only 5 leaves
        let split_keys = gen_keys(20);
        let parts = tree.split_many(&split_keys, &store)?;

        assert_eq!(parts.len(), 21);

        // Verify all original keys preserved
        let mut all_keys = Vec::new();
        for part in parts.iter().flatten() {
            all_keys.extend(collect_keys(part, &store)?);
        }
        all_keys.sort();
        assert_eq!(all_keys, keys);

        Ok(())
    }

    // ==================== Adjacent key edge cases ====================

    #[test]
    fn split_adjacent_keys() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();

        // Create leaves with adjacent UUIDs
        let key1 = {
            let mut bytes = [0u8; 16];
            bytes[15] = 1;
            UUID::from_bytes(bytes)
        };
        let key2 = {
            let mut bytes = [0u8; 16];
            bytes[15] = 2;
            UUID::from_bytes(bytes)
        };
        let key3 = {
            let mut bytes = [0u8; 16];
            bytes[15] = 3;
            UUID::from_bytes(bytes)
        };

        let all_keys = vec![key1, key2, key3];
        let tree = make_tree(&all_keys, &store)?;

        // Split at key2
        let parts = tree.split_many(&[key2], &store)?;

        let left_keys = parts[0]
            .as_ref()
            .map(|t| collect_keys(t, &store))
            .transpose()?
            .unwrap_or_default();
        let right_keys = parts[1]
            .as_ref()
            .map(|t| collect_keys(t, &store))
            .transpose()?
            .unwrap_or_default();

        assert_eq!(left_keys, vec![key1]); // Only key1 < key2
        assert_eq!(right_keys, vec![key2, key3]); // key2, key3 >= key2

        Ok(())
    }

    #[test]
    fn split_with_gaps_in_key_space() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();

        // Create keys with large gaps
        let key_low = UUID::nil();
        let key_high = UUID::max();
        let keys = vec![key_low, key_high];
        let tree = make_tree(&keys, &store)?;

        // Split in the middle of the gap
        let split_key = {
            let bytes = [0x80u8; 16];
            UUID::from_bytes(bytes)
        };

        let parts = tree.split_many(&[split_key], &store)?;

        let left = parts[0].as_ref().ok_or("expected left")?;
        let right = parts[1].as_ref().ok_or("expected right")?;

        assert_eq!(collect_keys(left, &store)?, vec![key_low]);
        assert_eq!(collect_keys(right, &store)?, vec![key_high]);

        Ok(())
    }

    // ==================== Empty tree edge cases ====================

    #[test]
    fn split_empty_tree_with_many_keys() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let tree: HtreeNode<u64> = HtreeNode::default();
        let split_keys = gen_keys(100);

        let parts = tree.split_many(&split_keys, &store)?;

        assert_eq!(parts.len(), 101);
        assert!(parts.iter().all(std::option::Option::is_none));

        Ok(())
    }

    #[test]
    fn split_empty_tree_with_one_key() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let tree: HtreeNode<u64> = HtreeNode::default();
        let split_key = UUID::gen_v4();

        let parts = tree.split_many(&[split_key], &store)?;

        assert_eq!(parts.len(), 2);
        assert!(parts[0].is_none());
        assert!(parts[1].is_none());

        Ok(())
    }

    // ==================== Partition correctness edge cases ====================

    #[test]
    fn all_keys_go_to_first_partition() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(20);
        let tree = make_tree(&keys, &store)?;

        // All split keys greater than all tree keys
        let split_keys = vec![UUID::max()];
        let parts = tree.split_many(&split_keys, &store)?;

        let first = parts[0].as_ref().ok_or("expected first")?;
        assert!(parts[1].is_none());
        assert_eq!(collect_keys(first, &store)?, keys);

        Ok(())
    }

    #[test]
    fn all_keys_go_to_last_partition() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(20);
        let tree = make_tree(&keys, &store)?;

        // All split keys less than all tree keys
        let split_keys = vec![UUID::nil()];
        let parts = tree.split_many(&split_keys, &store)?;

        assert!(parts[0].is_none());
        let last = parts[1].as_ref().ok_or("expected last")?;
        assert_eq!(collect_keys(last, &store)?, keys);

        Ok(())
    }

    #[test]
    fn each_key_in_separate_partition() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(10);
        let tree = make_tree(&keys, &store)?;

        // Split at each key - first partition empty, then one key each
        let parts = tree.split_many(&keys, &store)?;

        assert_eq!(parts.len(), 11);
        assert!(parts[0].is_none()); // No key < first key

        for i in 1..=10 {
            let part = parts[i]
                .as_ref()
                .ok_or_else(|| format!("expected partition {i}"))?;
            let part_keys = collect_keys(part, &store)?;
            assert_eq!(part_keys.len(), 1);
            assert_eq!(part_keys[0], keys[i - 1]);
        }

        Ok(())
    }

    // ==================== Rebuild tree edge cases ====================

    #[test]
    fn rebuild_empty_vec_returns_none() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();

        // Create tree then split with keys that produce empty partition
        let key = UUID::gen_v4();
        let tree = HtreeNode::<u64>::from_kvp(&key, &42, &store)?;

        // Split key less than the leaf - first partition empty
        let parts = tree.split_many(&[key], &store)?;

        assert!(parts[0].is_none()); // Empty partition rebuilt as None

        Ok(())
    }

    #[test]
    fn rebuild_with_varying_heights() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();

        // Create a complex tree that will produce varying heights when split
        let mut all_keys = Vec::new();

        // Dense region (will create deeper subtree)
        for i in 0..100 {
            let mut bytes = [0u8; 16];
            bytes[15] = i;
            all_keys.push(UUID::from_bytes(bytes));
        }

        // Sparse region at end
        let bytes = [0xFFu8; 16];
        all_keys.push(UUID::from_bytes(bytes));

        all_keys.sort();
        let tree = make_tree(&all_keys, &store)?;

        // Split to separate dense and sparse regions
        let split_key = {
            let bytes = [0x80u8; 16];
            UUID::from_bytes(bytes)
        };

        let parts = tree.split_many(&[split_key], &store)?;

        // Both parts should be valid
        let left = parts[0].as_ref().ok_or("expected left")?;
        let right = parts[1].as_ref().ok_or("expected right")?;

        // Heights might differ but both should be valid trees
        let left_keys = collect_keys(left, &store)?;
        let right_keys = collect_keys(right, &store)?;

        assert_eq!(left_keys.len(), 100);
        assert_eq!(right_keys.len(), 1);

        Ok(())
    }

    // ==================== Split key routing ====================

    #[test]
    fn split_key_exactly_at_child_key() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(100);
        let tree = make_tree(&keys, &store)?;

        // Get a child's key
        let children = tree.fetch_children(&store)?;
        let child_key = children[children.len() / 2].key;

        let parts = tree.split_many(&[child_key], &store)?;

        // Verify boundaries
        let left_keys = parts[0]
            .as_ref()
            .map(|t| collect_keys(t, &store))
            .transpose()?
            .unwrap_or_default();
        let right_keys = parts[1]
            .as_ref()
            .map(|t| collect_keys(t, &store))
            .transpose()?
            .unwrap_or_default();

        for key in &left_keys {
            assert!(key < &child_key);
        }
        for key in &right_keys {
            assert!(key >= &child_key);
        }

        // All keys preserved
        let mut all_keys: Vec<_> = left_keys.into_iter().chain(right_keys).collect();
        all_keys.sort();
        assert_eq!(all_keys, keys);

        Ok(())
    }

    #[test]
    fn split_keys_span_multiple_children() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(200);
        let tree = make_tree(&keys, &store)?;

        // Get several child keys
        let children = tree.fetch_children(&store)?;
        assert!(children.len() >= 3);

        let split_keys: Vec<UUID> = children.iter().take(3).map(|c| c.key).collect();
        let parts = tree.split_many(&split_keys, &store)?;

        assert_eq!(parts.len(), 4);
        assert_partition_boundaries(&parts, &split_keys, &store)?;

        Ok(())
    }

    // ==================== Randomized stress tests ====================

    #[test]
    fn random_splits_preserve_all_keys() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();

        for _ in 0..5 {
            let num_keys = 50 + (UUID::gen_v4().as_bytes()[0] as usize % 100);
            let keys = gen_keys(num_keys);
            let tree = make_tree(&keys, &store)?;

            let num_splits = 1 + (UUID::gen_v4().as_bytes()[0] as usize % 20);
            let split_keys: Vec<UUID> = (0..num_splits)
                .map(|_| {
                    let idx = UUID::gen_v4().as_bytes()[0] as usize % num_keys;
                    keys[idx]
                })
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect();

            let parts = tree.split_many(&split_keys, &store)?;

            let mut all_recovered = Vec::new();
            for part in parts.iter().flatten() {
                all_recovered.extend(collect_keys(part, &store)?);
            }
            all_recovered.sort();
            assert_eq!(all_recovered, keys);
        }

        Ok(())
    }

    #[test]
    fn random_splits_respect_boundaries() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();

        for _ in 0..5 {
            let num_keys = 30 + (UUID::gen_v4().as_bytes()[0] as usize % 50);
            let keys = gen_keys(num_keys);
            let tree = make_tree(&keys, &store)?;

            let mut split_keys = gen_keys(5);
            split_keys.sort();

            let parts = tree.split_many(&split_keys, &store)?;
            assert_partition_boundaries(&parts, &split_keys, &store)?;
        }

        Ok(())
    }

    // ==================== Specific regression tests ====================

    #[test]
    fn split_at_boundary_does_not_lose_boundary_key() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(20);
        let tree = make_tree(&keys, &store)?;

        // Split exactly at an existing key
        let split_key = keys[10];
        let parts = tree.split_many(&[split_key], &store)?;

        // The split key should be in the right partition (>= split_key)
        let right_keys = parts[1]
            .as_ref()
            .map(|t| collect_keys(t, &store))
            .transpose()?
            .unwrap_or_default();

        assert!(
            right_keys.contains(&split_key),
            "Split key should be in right partition"
        );

        Ok(())
    }

    #[test]
    fn split_preserves_tree_validity() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(100);
        let tree = make_tree(&keys, &store)?;

        let split_keys: Vec<UUID> = keys.iter().step_by(20).copied().collect();
        let parts = tree.split_many(&split_keys, &store)?;

        // Each non-empty partition should be a valid tree
        for (i, part) in parts.iter().enumerate() {
            if let Some(tree) = part {
                // Should be able to iterate without errors
                let keys: Result<Vec<_>, _> = tree.iter_keys(&store).collect();
                assert!(keys.is_ok(), "Partition {i} should be iterable");

                // Height should make sense
                if tree.is_leaf() {
                    assert_eq!(tree.height, 0);
                } else {
                    assert!(tree.height > 0);
                }
            }
        }

        Ok(())
    }

    // ==================== Leaf partition_point logic ====================

    #[test]
    fn leaf_partition_point_at_key() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();

        // Single leaf - test partition_point behavior
        let leaf_key = {
            let bytes = [0x50u8; 16];
            UUID::from_bytes(bytes)
        };
        let tree = HtreeNode::<u64>::from_kvp(&leaf_key, &42, &store)?;

        // Split keys: one less than, one equal, one greater
        let key_less = {
            let bytes = [0x40u8; 16];
            UUID::from_bytes(bytes)
        };
        let key_equal = leaf_key;
        let key_greater = {
            let bytes = [0x60u8; 16];
            UUID::from_bytes(bytes)
        };

        // Test with key_less
        let parts = tree.split_many(&[key_less], &store)?;
        assert!(parts[0].is_none());
        assert!(parts[1].is_some()); // leaf >= key_less

        // Test with key_equal
        let parts = tree.split_many(&[key_equal], &store)?;
        assert!(parts[0].is_none()); // nothing < leaf_key
        assert!(parts[1].is_some()); // leaf >= key_equal

        // Test with key_greater
        let parts = tree.split_many(&[key_greater], &store)?;
        assert!(parts[0].is_some()); // leaf < key_greater
        assert!(parts[1].is_none());

        Ok(())
    }

    #[test]
    fn leaf_partition_point_multiple_keys() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();

        let leaf_key = {
            let bytes = [0x50u8; 16];
            UUID::from_bytes(bytes)
        };
        let tree = HtreeNode::<u64>::from_kvp(&leaf_key, &42, &store)?;

        // Multiple split keys, leaf should go to correct partition
        let split_keys = vec![
            {
                let bytes = [0x30u8; 16];
                UUID::from_bytes(bytes)
            },
            {
                let bytes = [0x40u8; 16];
                UUID::from_bytes(bytes)
            },
            {
                let bytes = [0x60u8; 16];
                UUID::from_bytes(bytes)
            },
        ];

        let parts = tree.split_many(&split_keys, &store)?;

        assert_eq!(parts.len(), 4);
        assert!(parts[0].is_none()); // < 0x30
        assert!(parts[1].is_none()); // [0x30, 0x40)
        assert!(parts[2].is_some()); // [0x40, 0x60) - leaf is here (0x50)
        assert!(parts[3].is_none()); // >= 0x60

        Ok(())
    }
}
