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
            .ok_or("expected at least one node".into())
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
        assert!(parts.iter().all(|p| p.is_none()));
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
                    .map(|opt| opt.unwrap_or_default())
            })
            .collect::<Result<_, _>>()?;

        let seq_keys: Vec<Vec<UUID>> = [p0, p1, p2]
            .iter()
            .map(|p| {
                p.as_ref()
                    .map(|t| collect_keys(t, &store))
                    .transpose()
                    .map(|opt| opt.unwrap_or_default())
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
            let tree = part.as_ref().ok_or(format!("expected partition {i}"))?;
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
                    .map(|opt| opt.unwrap_or_default())
            })
            .collect::<Result<_, _>>()?;

        let seq_keys: Vec<Vec<UUID>> = [p0, p1, p2, p3, p4]
            .iter()
            .map(|p| {
                p.as_ref()
                    .map(|t| collect_keys(t, &store))
                    .transpose()
                    .map(|opt| opt.unwrap_or_default())
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
}
