use std::collections::{HashSet, VecDeque};

use ps_hkey::Store;
use ps_uuid::UUID;

use crate::HtreeNode;

impl<T> HtreeNode<T> {
    /// Merges two trees into one, producing a tree containing the union of
    /// all leaves from both `self` and `other`.
    ///
    /// # Arguments
    ///
    /// * `other` - The other tree to merge with.
    /// * `store` - Persistence backend.
    ///
    /// # Errors
    ///
    /// - [`HtreeNodeMergeError::CorruptedState`] if the node's internal state is invalid.
    /// - [`HtreeNodeMergeError::FromChildren`] if node reconstruction fails.
    /// - [`HtreeNodeMergeError::Store`] if store operations fail.
    /// - [`HtreeNodeMergeError::UnpackChildren`] if child deserialization fails.
    pub fn merge<S>(self, other: Self, store: &S) -> Result<Self, HtreeNodeMergeError<S>>
    where
        S: Store,
    {
        let mut merged = Self::merge_many([self, other], store)?;

        if merged.len() <= 1 {
            return Ok(merged.pop().unwrap_or_default());
        }

        Self::from_children(merged, store).map_err(Into::into)
    }

    /// Merges multiple trees into one, producing a tree containing the union
    /// of all leaves from every input tree.
    ///
    /// # Arguments
    ///
    /// * `nodes` - The trees to merge.
    /// * `store` - Persistence backend.
    ///
    /// # Errors
    ///
    /// - [`HtreeNodeMergeError::CorruptedState`] if the node's internal state is invalid.
    /// - [`HtreeNodeMergeError::FromChildren`] if node reconstruction fails.
    /// - [`HtreeNodeMergeError::Store`] if store operations fail.
    /// - [`HtreeNodeMergeError::UnpackChildren`] if child deserialization fails.
    pub fn merge_many<I, S>(nodes: I, store: &S) -> Result<Vec<Self>, HtreeNodeMergeError<S>>
    where
        I: IntoIterator<Item = Self>,
        S: Store,
    {
        // Filter out empty nodes upfront
        let nodes: VecDeque<Self> = nodes.into_iter().filter(|n| !n.is_empty()).collect();

        let Some(base) = nodes.iter().max_by(|a, b| a.height().cmp(&b.height())) else {
            // All inputs were empty
            return Ok(Vec::default());
        };

        // For internal nodes, fetch their children.
        // For leaves, start empty - the base leaf will be added via the normal loop.
        let mut children = if base.is_leaf() {
            Vec::new()
        } else {
            base.fetch_children(store)?
        };

        // Track seen internal nodes by hkey for structural sharing.
        // Leaves are not tracked here because their hkeys may not include the key,
        // only the value - so different leaves can have the same hkey.
        let mut seen = HashSet::new();

        // Only add internal nodes to seen (not leaves)
        if !base.is_leaf() {
            seen.insert(base.hkey.clone());
            for child in &children {
                if !child.is_leaf() {
                    seen.insert(child.hkey.clone());
                }
            }
        }

        // Phase 1: Separate leaves from internal nodes and filter by seen
        let mut leaves: Vec<Self> = Vec::new();
        let mut internal_queue: VecDeque<Self> = VecDeque::new();

        // Empty nodes were already filtered at the start, so no need to check here.
        for node in nodes {
            if node.is_leaf() {
                leaves.push(node);
            } else if !seen.contains(&node.hkey) {
                seen.insert(node.hkey.clone());
                internal_queue.push_back(node);
            }
        }

        // Phase 2: Process internal nodes - unpack tall ones, collect same-height ones
        let mut same_height_nodes: Vec<Self> = Vec::new();

        while let Some(node) = internal_queue.pop_front() {
            let child_height = children.first().map_or(0, Self::height);

            if node.height() > child_height {
                // Taller than children - unpack and categorize
                for child in node.fetch_children(store)? {
                    if child.is_empty() {
                        continue;
                    }
                    if child.is_leaf() {
                        leaves.push(child);
                    } else if !seen.contains(&child.hkey) {
                        seen.insert(child.hkey.clone());
                        internal_queue.push_back(child);
                    }
                }
            } else {
                // Same height as children - collect for batch processing
                same_height_nodes.push(node);
            }
        }

        // Phase 3: Batch-merge all same-height internal nodes at once
        if !same_height_nodes.is_empty() {
            children = Self::merge_at_level_batched(children, same_height_nodes, store)?;

            // Add new internal children to seen
            for child in &children {
                if !child.is_leaf() {
                    seen.insert(child.hkey.clone());
                }
            }
        }

        // Phase 4: Batch-merge all leaves
        if !leaves.is_empty() {
            if children.is_empty() || children[0].is_leaf() {
                // Children are leaves (or absent) — insert directly via binary search
                for leaf in leaves {
                    let idx = children.partition_point(|child| child.key < leaf.key);
                    if idx < children.len() && children[idx].key == leaf.key {
                        children[idx] = leaf;
                    } else {
                        children.insert(idx, leaf);
                    }
                }
            } else {
                // Children are internal nodes — batch leaves by target child.
                // partition_point returns the first index where child.key > leaf.key,
                // then saturating_sub(1) gives the last child where child.key <= leaf.key.
                // Edge case: if leaf.key < children[0].key, partition_point returns 0,
                // saturating_sub(1) gives 0, so the leaf goes to the first child (correct).
                let mut leaf_batches: Vec<Vec<Self>> = vec![Vec::new(); children.len()];
                for leaf in leaves {
                    let idx = children
                        .partition_point(|child| child.key <= leaf.key)
                        .saturating_sub(1);
                    leaf_batches[idx].push(leaf);
                }

                // Merge each child with all its batched leaves in ONE recursive call
                let mut new_children = Vec::with_capacity(children.len());
                for (child, batch) in children.into_iter().zip(leaf_batches.into_iter()) {
                    if batch.is_empty() {
                        new_children.push(child);
                    } else {
                        let merged = Self::merge_many(std::iter::once(child).chain(batch), store)?;
                        new_children.extend(merged);
                    }
                }
                children = new_children;
            }
        }

        Self::from_many_children(children, store).map_err(Into::into)
    }

    /// Merges multiple nodes into an existing set of children by splitting each node
    /// by child boundaries, accumulating all partitions, then recursively merging
    /// each child with ALL its accumulated partitions in a single call.
    ///
    /// This batched approach reduces O(N * H) to O(M * H) where M = number of trees.
    fn merge_at_level_batched<S>(
        children: Vec<Self>,
        nodes: Vec<Self>,
        store: &S,
    ) -> Result<Vec<Self>, HtreeNodeMergeError<S>>
    where
        S: Store,
    {
        if children.is_empty() {
            // No children yet - fetch children from all nodes and merge them
            let mut all_children: Vec<Self> = Vec::new();

            for node in nodes {
                all_children.extend(node.fetch_children(store)?);
            }

            return Self::merge_many(all_children, store);
        }

        // Compute split keys: the keys of children[1..] define partition boundaries
        // Partition i contains all leaves with key in [children[i].key, children[i+1].key)
        let split_keys: Vec<UUID> = children.iter().skip(1).map(|c| c.key).collect();

        // Accumulate partitions for each child from ALL incoming nodes
        let mut partition_batches: Vec<Vec<Self>> = vec![Vec::new(); children.len()];

        for node in nodes {
            // Split this node into partitions matching the children
            let partitions = node.split_many(&split_keys, store)?;

            for (i, partition) in partitions.into_iter().enumerate() {
                if let Some(part) = partition {
                    partition_batches[i].push(part);
                }
            }
        }

        // Merge each child with ALL its accumulated partitions in ONE recursive call
        let mut result = Vec::with_capacity(children.len());

        for (child, batch) in children.into_iter().zip(partition_batches.into_iter()) {
            if batch.is_empty() {
                // No overlap - keep child as-is
                result.push(child);
            } else {
                // Recursively merge child with all partitions at once
                let merged = Self::merge_many(std::iter::once(child).chain(batch), store)?;
                result.extend(merged);
            }
        }

        Ok(result)
    }
}

#[derive(thiserror::Error, Debug)]
pub enum HtreeNodeMergeError<S: Store> {
    #[error("HtreeNode's state is corrupted.")]
    CorruptedState,

    #[error("Node reconstruction failed.")]
    FromChildren(crate::HtreeNodeFromChildrenError<S>),

    #[error("Store error: {0}")]
    Store(S::Error),

    #[error("Error unpacking children: {0}")]
    UnpackChildren(#[from] crate::HtreeNodeUnpackChildrenError),
}

impl<S: Store> From<crate::HtreeNodeFetchChildrenError<S>> for HtreeNodeMergeError<S> {
    fn from(value: crate::HtreeNodeFetchChildrenError<S>) -> Self {
        match value {
            crate::HtreeNodeFetchChildrenError::CorruptedState => Self::CorruptedState,
            crate::HtreeNodeFetchChildrenError::Store(err) => Self::Store(err),
            crate::HtreeNodeFetchChildrenError::UnpackChildren(err) => Self::UnpackChildren(err),
        }
    }
}

impl<S: Store> From<crate::HtreeNodeFromChildrenError<S>> for HtreeNodeMergeError<S> {
    fn from(value: crate::HtreeNodeFromChildrenError<S>) -> Self {
        match value {
            crate::HtreeNodeFromChildrenError::Store(err) => Self::Store(err),
            err => Self::FromChildren(err),
        }
    }
}

impl<S: Store> From<crate::HtreeNodeSplitManyError<S>> for HtreeNodeMergeError<S> {
    fn from(value: crate::HtreeNodeSplitManyError<S>) -> Self {
        match value {
            super::HtreeNodeSplitManyError::CorruptedState => Self::CorruptedState,
            super::HtreeNodeSplitManyError::FromChildren(err) => err.into(),
            super::HtreeNodeSplitManyError::Store(err) => Self::Store(err),
            super::HtreeNodeSplitManyError::UnpackChildren(err) => err.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use ps_datachunk::{DataChunk, OwnedDataChunk, PsDataChunkError};
    use ps_hash::Hash;
    use ps_hkey::{InMemoryStore, InMemoryStoreError, PsHkeyError, Store};
    use ps_uuid::UUID;

    use crate::HtreeNode;

    /// A wrapper store that counts read (`get`) and write (`put_encrypted`) operations.
    /// Used for verifying merge complexity.
    #[derive(Debug)]
    struct CountingStore {
        inner: InMemoryStore,
        reads: AtomicUsize,
        writes: AtomicUsize,
    }

    impl CountingStore {
        fn new() -> Self {
            Self {
                inner: InMemoryStore::default(),
                reads: AtomicUsize::new(0),
                writes: AtomicUsize::new(0),
            }
        }

        fn reads(&self) -> usize {
            self.reads.load(Ordering::Relaxed)
        }

        fn writes(&self) -> usize {
            self.writes.load(Ordering::Relaxed)
        }

        fn total_ops(&self) -> usize {
            self.reads() + self.writes()
        }

        fn reset_counts(&self) {
            self.reads.store(0, Ordering::Relaxed);
            self.writes.store(0, Ordering::Relaxed);
        }
    }

    #[derive(thiserror::Error, Debug)]
    pub enum CountingStoreError {
        #[error(transparent)]
        Inner(#[from] InMemoryStoreError),
    }

    impl From<PsDataChunkError> for CountingStoreError {
        fn from(err: PsDataChunkError) -> Self {
            Self::Inner(InMemoryStoreError::from(err))
        }
    }

    impl From<PsHkeyError> for CountingStoreError {
        fn from(err: PsHkeyError) -> Self {
            Self::Inner(InMemoryStoreError::from(err))
        }
    }

    impl Store for CountingStore {
        type Chunk<'c> = OwnedDataChunk;
        type Error = CountingStoreError;

        fn get<'a>(&'a self, hash: &Hash) -> Result<Self::Chunk<'a>, Self::Error> {
            self.reads.fetch_add(1, Ordering::Relaxed);
            self.inner.get(hash).map_err(Into::into)
        }

        fn put_encrypted<C: DataChunk>(&self, chunk: C) -> Result<(), Self::Error> {
            self.writes.fetch_add(1, Ordering::Relaxed);
            self.inner.put_encrypted(chunk).map_err(Into::into)
        }
    }

    /// Collects all keys from a tree in sorted order.
    fn collect_keys(
        tree: &HtreeNode<u64>,
        store: &InMemoryStore,
    ) -> Result<Vec<UUID>, Box<dyn std::error::Error>> {
        Ok(tree.iter_keys(store).collect::<Result<Vec<_>, _>>()?)
    }

    /// Collects all keys from multiple trees in sorted order.
    fn collect_keys_many(
        trees: &[HtreeNode<u64>],
        store: &InMemoryStore,
    ) -> Result<Vec<UUID>, Box<dyn std::error::Error>> {
        let mut keys = Vec::new();
        for tree in trees {
            keys.extend(tree.iter_keys(store).collect::<Result<Vec<_>, _>>()?);
        }
        keys.sort();
        Ok(keys)
    }

    /// Builds a single-root tree from the given keys (with dummy u64 values).
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

    /// Generates `n` random sorted UUIDs.
    fn gen_keys(n: usize) -> Vec<UUID> {
        let mut keys: Vec<UUID> = (0..n).map(|_| UUID::gen_v4()).collect();
        keys.sort();
        keys
    }

    // -- empty trees --

    #[test]
    fn merge_empty_trees() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let a: HtreeNode<u64> = HtreeNode::default();
        let b: HtreeNode<u64> = HtreeNode::default();

        let merged = a.merge(b, &store)?;
        assert!(merged.is_empty());
        Ok(())
    }

    #[test]
    fn merge_many_empty() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let merged = HtreeNode::<u64>::merge_many(std::iter::empty(), &store)?;
        assert!(merged.is_empty());
        Ok(())
    }

    // -- single tree --

    #[test]
    fn merge_single_tree_preserves_keys() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(10);
        let tree = make_tree(&keys, &store)?;

        let merged = HtreeNode::merge_many([tree], &store)?;
        let merged_keys = collect_keys_many(&merged, &store)?;

        assert_eq!(merged_keys, keys);
        Ok(())
    }

    // -- with empty --

    #[test]
    fn merge_with_empty_preserves_keys() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(10);
        let tree = make_tree(&keys, &store)?;
        let empty: HtreeNode<u64> = HtreeNode::default();

        let merged = tree.merge(empty, &store)?;
        let merged_keys = collect_keys(&merged, &store)?;

        assert_eq!(merged_keys, keys);
        Ok(())
    }

    // -- disjoint trees --

    #[test]
    fn merge_disjoint_trees() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let all_keys = gen_keys(20);
        let a = make_tree(&all_keys[..10], &store)?;
        let b = make_tree(&all_keys[10..], &store)?;

        let merged = a.merge(b, &store)?;
        let merged_keys = collect_keys(&merged, &store)?;

        assert_eq!(merged_keys, all_keys);
        Ok(())
    }

    // -- split then merge --

    #[test]
    fn merge_after_split_restores_keys() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(20);
        let tree = make_tree(&keys, &store)?;

        let (lt, gte) = tree.split_at(&keys[10], &store)?;
        let lt = lt.ok_or("expected lesser")?;
        let gte = gte.ok_or("expected greater")?;

        let merged = lt.merge(gte, &store)?;
        let merged_keys = collect_keys(&merged, &store)?;

        assert_eq!(merged_keys, keys);
        Ok(())
    }

    #[test]
    fn merge_after_split_at_every_position() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(15);
        let tree = make_tree(&keys, &store)?;

        for &split_key in &keys {
            let (lt, gte) = tree.split_at(&split_key, &store)?;

            let parts: Vec<HtreeNode<u64>> = [lt, gte].into_iter().flatten().collect();
            let merged = HtreeNode::merge_many(parts, &store)?;
            let merged_keys = collect_keys_many(&merged, &store)?;

            assert_eq!(
                merged_keys, keys,
                "merge failed after split at {split_key:?}"
            );
        }
        Ok(())
    }

    // -- overlapping trees --

    #[test]
    fn merge_overlapping_trees() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(20);

        // a has keys[0..15], b has keys[5..20] -- overlap at [5..15]
        let a = make_tree(&keys[..15], &store)?;
        let b = make_tree(&keys[5..], &store)?;

        let merged = a.merge(b, &store)?;
        let merged_keys = collect_keys(&merged, &store)?;

        assert_eq!(merged_keys, keys);
        Ok(())
    }

    // -- identical trees --

    #[test]
    fn merge_identical_trees() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(10);
        let a = make_tree(&keys, &store)?;
        let b = make_tree(&keys, &store)?;

        let merged = a.merge(b, &store)?;
        let merged_keys = collect_keys(&merged, &store)?;

        assert_eq!(merged_keys, keys);
        Ok(())
    }

    // -- different heights --

    #[test]
    fn merge_leaf_with_tall_tree() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(21);
        let tree = make_tree(&keys[..20], &store)?;
        let leaf = HtreeNode::<u64>::from_kvp(&keys[20], &99, &store)?;

        let merged = tree.merge(leaf, &store)?;
        let merged_keys = collect_keys(&merged, &store)?;

        assert_eq!(merged_keys, keys);
        Ok(())
    }

    #[test]
    fn merge_trees_of_different_heights() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(25);

        // small tree (few keys, low height) and large tree (many keys, higher height)
        let small = make_tree(&keys[..3], &store)?;
        let large = make_tree(&keys[3..], &store)?;

        let merged = small.merge(large, &store)?;
        let merged_keys = collect_keys(&merged, &store)?;

        assert_eq!(merged_keys, keys);
        Ok(())
    }

    // -- merge_many with multiple trees --

    #[test]
    fn merge_many_disjoint() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(30);

        let a = make_tree(&keys[..10], &store)?;
        let b = make_tree(&keys[10..20], &store)?;
        let c = make_tree(&keys[20..], &store)?;

        let merged = HtreeNode::merge_many([a, b, c], &store)?;
        let merged_keys = collect_keys_many(&merged, &store)?;

        assert_eq!(merged_keys, keys);
        Ok(())
    }

    // -- three-way split and merge --

    #[test]
    fn three_way_split_and_merge() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(30);
        let tree = make_tree(&keys, &store)?;

        let (lt, rest) = tree.split_at(&keys[10], &store)?;
        let rest = rest.ok_or("expected rest")?;
        let (mid, gte) = rest.split_at(&keys[20], &store)?;

        let parts: Vec<HtreeNode<u64>> = [lt, mid, gte].into_iter().flatten().collect();
        let merged = HtreeNode::merge_many(parts, &store)?;
        let merged_keys = collect_keys_many(&merged, &store)?;

        assert_eq!(merged_keys, keys);
        Ok(())
    }

    // -- large tree --

    #[test]
    fn large_merge_preserves_all_keys() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(100);
        let tree = make_tree(&keys, &store)?;

        let (lt, gte) = tree.split_at(&keys[50], &store)?;
        let lt = lt.ok_or("expected lesser")?;
        let gte = gte.ok_or("expected greater")?;

        let merged = lt.merge(gte, &store)?;
        let merged_keys = collect_keys(&merged, &store)?;

        assert_eq!(merged_keys, keys);
        Ok(())
    }

    // -- find_one on merged tree --

    #[test]
    fn find_one_works_on_merged_tree() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(20);
        let tree = make_tree(&keys, &store)?;

        let (lt, gte) = tree.split_at(&keys[10], &store)?;
        let lt = lt.ok_or("expected lesser")?;
        let gte = gte.ok_or("expected greater")?;

        let merged = lt.merge(gte, &store)?;

        for &k in &keys {
            assert!(
                merged.find_one(&k, &store)?.is_some(),
                "key {k:?} not found in merged tree"
            );
        }
        Ok(())
    }

    // =========================================================================
    // SIGNIFICANTLY SIMILAR TREES
    // =========================================================================

    #[test]
    fn merge_trees_with_90_percent_overlap() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(100);

        // a has keys[0..95], b has keys[5..100] -- 90% overlap at [5..95]
        let a = make_tree(&keys[..95], &store)?;
        let b = make_tree(&keys[5..], &store)?;

        let merged = a.merge(b, &store)?;
        let merged_keys = collect_keys(&merged, &store)?;

        assert_eq!(merged_keys, keys);
        Ok(())
    }

    #[test]
    fn merge_trees_with_99_percent_overlap() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(100);

        // a has keys[0..100], b has keys[1..100] -- 99% overlap
        let a = make_tree(&keys, &store)?;
        let b = make_tree(&keys[1..], &store)?;

        let merged = a.merge(b, &store)?;
        let merged_keys = collect_keys(&merged, &store)?;

        assert_eq!(merged_keys, keys);
        Ok(())
    }

    #[test]
    fn merge_nearly_identical_trees_differ_by_one_leaf() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(51);

        // a has all but the middle key, b has all but the first key
        let mut a_keys: Vec<UUID> = keys[..25].to_vec();
        a_keys.extend_from_slice(&keys[26..]);

        let b_keys: Vec<UUID> = keys[1..].to_vec();

        let a = make_tree(&a_keys, &store)?;
        let b = make_tree(&b_keys, &store)?;

        let merged = a.merge(b, &store)?;
        let merged_keys = collect_keys(&merged, &store)?;

        assert_eq!(merged_keys, keys);
        Ok(())
    }

    #[test]
    fn merge_tree_with_itself() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(50);
        let tree = make_tree(&keys, &store)?;

        let merged = tree.clone().merge(tree.clone(), &store)?;
        let merged_keys = collect_keys(&merged, &store)?;

        assert_eq!(merged_keys, keys);
        Ok(())
    }

    #[test]
    fn merge_subset_into_superset() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(50);

        let superset = make_tree(&keys, &store)?;
        let subset = make_tree(&keys[10..40], &store)?;

        let merged = superset.merge(subset, &store)?;
        let merged_keys = collect_keys(&merged, &store)?;

        assert_eq!(merged_keys, keys);
        Ok(())
    }

    #[test]
    fn merge_superset_into_subset() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(50);

        let superset = make_tree(&keys, &store)?;
        let subset = make_tree(&keys[10..40], &store)?;

        // Reverse order from previous test
        let merged = subset.merge(superset, &store)?;
        let merged_keys = collect_keys(&merged, &store)?;

        assert_eq!(merged_keys, keys);
        Ok(())
    }

    // =========================================================================
    // SIGNIFICANTLY DIVERGENT TREES
    // =========================================================================

    #[test]
    fn merge_interleaved_keys_even_odd() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(100);

        // a has even indices, b has odd indices -- completely disjoint, interleaved
        let even_keys: Vec<UUID> = keys.iter().step_by(2).copied().collect();
        let odd_keys: Vec<UUID> = keys.iter().skip(1).step_by(2).copied().collect();

        let a = make_tree(&even_keys, &store)?;
        let b = make_tree(&odd_keys, &store)?;

        let merged = a.merge(b, &store)?;
        let merged_keys = collect_keys(&merged, &store)?;

        assert_eq!(merged_keys, keys);
        Ok(())
    }

    #[test]
    fn merge_interleaved_keys_thirds() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(90);

        // Split into three interleaved parts
        let a_keys: Vec<UUID> = keys.iter().step_by(3).copied().collect();
        let b_keys: Vec<UUID> = keys.iter().skip(1).step_by(3).copied().collect();
        let c_keys: Vec<UUID> = keys.iter().skip(2).step_by(3).copied().collect();

        let a = make_tree(&a_keys, &store)?;
        let b = make_tree(&b_keys, &store)?;
        let c = make_tree(&c_keys, &store)?;

        let merged = HtreeNode::merge_many([a, b, c], &store)?;
        let merged_keys = collect_keys_many(&merged, &store)?;

        assert_eq!(merged_keys, keys);
        Ok(())
    }

    #[test]
    fn merge_completely_disjoint_ranges() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(100);

        // a has first 25, b has last 25 -- large gap in the middle
        let a = make_tree(&keys[..25], &store)?;
        let b = make_tree(&keys[75..], &store)?;

        let merged = a.merge(b, &store)?;
        let merged_keys = collect_keys(&merged, &store)?;

        let mut expected: Vec<UUID> = keys[..25].to_vec();
        expected.extend_from_slice(&keys[75..]);
        assert_eq!(merged_keys, expected);
        Ok(())
    }

    #[test]
    fn merge_many_small_disjoint_ranges() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(100);

        // Create 10 trees, each with 10 consecutive keys from disjoint ranges
        let trees: Vec<HtreeNode<u64>> = (0..10)
            .map(|i| make_tree(&keys[i * 10..(i + 1) * 10], &store))
            .collect::<Result<Vec<_>, _>>()?;

        let merged = HtreeNode::merge_many(trees, &store)?;
        let merged_keys = collect_keys_many(&merged, &store)?;

        assert_eq!(merged_keys, keys);
        Ok(())
    }

    #[test]
    fn merge_alternating_blocks() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(100);

        // a has blocks 0-9, 20-29, 40-49, 60-69, 80-89
        // b has blocks 10-19, 30-39, 50-59, 70-79, 90-99
        let a_keys: Vec<UUID> = (0..5)
            .flat_map(|i| keys[i * 20..i * 20 + 10].iter().copied())
            .collect();
        let b_keys: Vec<UUID> = (0..5)
            .flat_map(|i| keys[i * 20 + 10..i * 20 + 20].iter().copied())
            .collect();

        let a = make_tree(&a_keys, &store)?;
        let b = make_tree(&b_keys, &store)?;

        let merged = a.merge(b, &store)?;
        let merged_keys = collect_keys(&merged, &store)?;

        assert_eq!(merged_keys, keys);
        Ok(())
    }

    // =========================================================================
    // HEIGHT EDGE CASES
    // =========================================================================

    #[test]
    fn merge_single_leaf_trees() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(2);

        let a = HtreeNode::<u64>::from_kvp(&keys[0], &0, &store)?;
        let b = HtreeNode::<u64>::from_kvp(&keys[1], &1, &store)?;

        let merged = a.merge(b, &store)?;
        let merged_keys = collect_keys(&merged, &store)?;

        assert_eq!(merged_keys, keys);
        Ok(())
    }

    #[test]
    fn merge_single_leaf_with_two_leaf_tree() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(3);

        let a = HtreeNode::<u64>::from_kvp(&keys[0], &0, &store)?;
        let b = make_tree(&keys[1..], &store)?;

        let merged = a.merge(b, &store)?;
        let merged_keys = collect_keys(&merged, &store)?;

        assert_eq!(merged_keys, keys);
        Ok(())
    }

    #[test]
    fn merge_trees_with_height_difference_of_two() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(505);

        // Create a very small tree (height 1) and a larger tree (height 3+)
        let small = make_tree(&keys[..5], &store)?;
        let large = make_tree(&keys[5..], &store)?;

        // The exact height difference depends on MAX_CHILDREN, but we want
        // to test merging trees with significantly different heights
        assert!(
            large.height() > small.height(),
            "large tree should be taller than small tree"
        );

        let merged = small.merge(large, &store)?;
        let merged_keys = collect_keys(&merged, &store)?;

        assert_eq!(merged_keys, keys);
        Ok(())
    }

    #[test]
    fn merge_trees_with_large_height_difference() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(202);

        let tiny = make_tree(&keys[..2], &store)?;
        let huge = make_tree(&keys[2..], &store)?;

        let merged = tiny.merge(huge, &store)?;
        let merged_keys = collect_keys(&merged, &store)?;

        assert_eq!(merged_keys, keys);
        Ok(())
    }

    // =========================================================================
    // COMMUTATIVITY AND ASSOCIATIVITY
    // =========================================================================

    #[test]
    fn merge_is_commutative_on_keys() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(50);

        let a = make_tree(&keys[..30], &store)?;
        let b = make_tree(&keys[20..], &store)?;

        let ab = a.clone().merge(b.clone(), &store)?;
        let ba = b.merge(a, &store)?;

        let ab_keys = collect_keys(&ab, &store)?;
        let ba_keys = collect_keys(&ba, &store)?;

        assert_eq!(ab_keys, ba_keys);
        Ok(())
    }

    #[test]
    fn merge_many_order_independent_keys() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(60);

        let a = make_tree(&keys[..20], &store)?;
        let b = make_tree(&keys[20..40], &store)?;
        let c = make_tree(&keys[40..], &store)?;

        let abc = HtreeNode::merge_many([a.clone(), b.clone(), c.clone()], &store)?;
        let cba = HtreeNode::merge_many([c.clone(), b.clone(), a.clone()], &store)?;
        let bac = HtreeNode::merge_many([b, a, c], &store)?;

        let abc_keys = collect_keys_many(&abc, &store)?;
        let cba_keys = collect_keys_many(&cba, &store)?;
        let bac_keys = collect_keys_many(&bac, &store)?;

        assert_eq!(abc_keys, cba_keys);
        assert_eq!(abc_keys, bac_keys);
        Ok(())
    }

    #[test]
    fn merge_associative_on_keys() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(60);

        let a = make_tree(&keys[..20], &store)?;
        let b = make_tree(&keys[20..40], &store)?;
        let c = make_tree(&keys[40..], &store)?;

        // (a merge b) merge c
        let ab = a.clone().merge(b.clone(), &store)?;
        let ab_c = ab.merge(c.clone(), &store)?;

        // a merge (b merge c)
        let bc = b.merge(c, &store)?;
        let a_bc = a.merge(bc, &store)?;

        let ab_c_keys = collect_keys(&ab_c, &store)?;
        let a_bc_keys = collect_keys(&a_bc, &store)?;

        assert_eq!(ab_c_keys, a_bc_keys);
        Ok(())
    }

    // =========================================================================
    // MERGE_MANY EDGE CASES
    // =========================================================================

    #[test]
    fn merge_many_with_empty_in_the_middle() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(30);

        let a = make_tree(&keys[..15], &store)?;
        let empty: HtreeNode<u64> = HtreeNode::default();
        let b = make_tree(&keys[15..], &store)?;

        let merged = HtreeNode::merge_many([a, empty, b], &store)?;
        let merged_keys = collect_keys_many(&merged, &store)?;

        assert_eq!(merged_keys, keys);
        Ok(())
    }

    #[test]
    fn merge_many_all_empty() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let empties: Vec<HtreeNode<u64>> = (0..5).map(|_| HtreeNode::default()).collect();

        let merged = HtreeNode::merge_many(empties, &store)?;
        assert!(merged.is_empty());
        Ok(())
    }

    #[test]
    fn merge_many_single_non_empty_among_empties() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(10);
        let tree = make_tree(&keys, &store)?;

        let trees: Vec<HtreeNode<u64>> = vec![
            HtreeNode::default(),
            HtreeNode::default(),
            tree,
            HtreeNode::default(),
        ];

        let merged = HtreeNode::merge_many(trees, &store)?;
        let merged_keys = collect_keys_many(&merged, &store)?;

        assert_eq!(merged_keys, keys);
        Ok(())
    }

    #[test]
    fn merge_many_with_varying_sizes() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(111);

        // Trees of sizes: 1, 10, 50, 50 (disjoint)
        let tiny = make_tree(&keys[..1], &store)?;
        let small = make_tree(&keys[1..11], &store)?;
        let medium = make_tree(&keys[11..61], &store)?;
        let large = make_tree(&keys[61..], &store)?;

        let merged = HtreeNode::merge_many([tiny, small, medium, large], &store)?;
        let merged_keys = collect_keys_many(&merged, &store)?;

        assert_eq!(merged_keys, keys);
        Ok(())
    }

    #[test]
    fn merge_many_ten_overlapping_trees() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(100);

        // Create 10 overlapping trees
        let trees: Vec<HtreeNode<u64>> = (0..10)
            .map(|i| {
                let start = i * 5;
                let end = (start + 60).min(100);
                make_tree(&keys[start..end], &store)
            })
            .collect::<Result<Vec<_>, _>>()?;

        let merged = HtreeNode::merge_many(trees, &store)?;
        let merged_keys = collect_keys_many(&merged, &store)?;

        assert_eq!(merged_keys, keys);
        Ok(())
    }

    // =========================================================================
    // SEQUENTIAL MERGES
    // =========================================================================

    #[test]
    fn sequential_merges_preserve_keys() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(80);

        let a = make_tree(&keys[..20], &store)?;
        let b = make_tree(&keys[20..40], &store)?;
        let c = make_tree(&keys[40..60], &store)?;
        let d = make_tree(&keys[60..], &store)?;

        let merged = a.merge(b, &store)?.merge(c, &store)?.merge(d, &store)?;
        let merged_keys = collect_keys(&merged, &store)?;

        assert_eq!(merged_keys, keys);
        Ok(())
    }

    #[test]
    fn sequential_merges_into_growing_tree() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(100);

        let mut merged = HtreeNode::<u64>::default();

        // Merge single leaves one by one
        for (i, key) in keys.iter().enumerate() {
            let leaf = HtreeNode::<u64>::from_kvp(key, &(i as u64), &store)?;
            merged = merged.merge(leaf, &store)?;
        }

        let merged_keys = collect_keys(&merged, &store)?;
        assert_eq!(merged_keys, keys);
        Ok(())
    }

    // =========================================================================
    // BOUNDARY KEYS
    // =========================================================================

    #[test]
    fn merge_with_min_max_keys() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let mut keys = gen_keys(10);
        keys.insert(0, UUID::nil());
        keys.push(UUID::max());
        keys.sort();

        let a = make_tree(&keys[..6], &store)?;
        let b = make_tree(&keys[6..], &store)?;

        let merged = a.merge(b, &store)?;
        let merged_keys = collect_keys(&merged, &store)?;

        assert_eq!(merged_keys, keys);
        Ok(())
    }

    #[test]
    fn merge_trees_both_containing_nil_key() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let mut a_keys = gen_keys(5);
        let mut b_keys = gen_keys(5);
        a_keys.insert(0, UUID::nil());
        b_keys.insert(0, UUID::nil());
        a_keys.sort();
        b_keys.sort();

        let a = make_tree(&a_keys, &store)?;
        let b = make_tree(&b_keys, &store)?;

        let merged = a.merge(b, &store)?;
        let merged_keys = collect_keys(&merged, &store)?;

        // Union should have nil exactly once
        let mut expected = a_keys.clone();
        expected.extend_from_slice(&b_keys);
        expected.sort();
        expected.dedup();
        assert_eq!(merged_keys, expected);
        Ok(())
    }

    // =========================================================================
    // STRESS TESTS
    // =========================================================================

    #[test]
    fn stress_merge_200_keys() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(200);

        let a = make_tree(&keys[..100], &store)?;
        let b = make_tree(&keys[100..], &store)?;

        let merged = a.merge(b, &store)?;
        let merged_keys = collect_keys(&merged, &store)?;

        assert_eq!(merged_keys, keys);
        Ok(())
    }

    #[test]
    fn stress_merge_many_20_trees() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(200);

        let trees: Vec<HtreeNode<u64>> = (0..20)
            .map(|i| make_tree(&keys[i * 10..(i + 1) * 10], &store))
            .collect::<Result<Vec<_>, _>>()?;

        let merged = HtreeNode::merge_many(trees, &store)?;
        let merged_keys = collect_keys_many(&merged, &store)?;

        assert_eq!(merged_keys, keys);
        Ok(())
    }

    #[test]
    fn stress_complex_overlapping_merge() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(150);

        // Create complex overlapping pattern
        let a = make_tree(&keys[0..100], &store)?;
        let b = make_tree(&keys[25..125], &store)?;
        let c = make_tree(&keys[50..150], &store)?;

        let merged = HtreeNode::merge_many([a, b, c], &store)?;
        let merged_keys = collect_keys_many(&merged, &store)?;

        assert_eq!(merged_keys, keys);
        Ok(())
    }

    // =========================================================================
    // FIND/ITERATION ON MERGED TREES
    // =========================================================================

    #[test]
    fn find_one_works_on_divergent_merge() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(100);

        let even: Vec<UUID> = keys.iter().step_by(2).copied().collect();
        let odd: Vec<UUID> = keys.iter().skip(1).step_by(2).copied().collect();

        let a = make_tree(&even, &store)?;
        let b = make_tree(&odd, &store)?;

        let merged = a.merge(b, &store)?;

        for &k in &keys {
            assert!(
                merged.find_one(&k, &store)?.is_some(),
                "key {k:?} not found"
            );
        }
        Ok(())
    }

    #[test]
    fn find_one_works_on_similar_merge() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(100);

        let a = make_tree(&keys[..95], &store)?;
        let b = make_tree(&keys[5..], &store)?;

        let merged = a.merge(b, &store)?;

        for &k in &keys {
            assert!(
                merged.find_one(&k, &store)?.is_some(),
                "key {k:?} not found"
            );
        }
        Ok(())
    }

    #[test]
    fn iter_keys_on_complex_merge() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(100);

        // Create 5 trees with different patterns
        let trees = vec![
            make_tree(&keys[..40], &store)?,
            make_tree(&keys[20..60], &store)?,
            make_tree(&keys[40..80], &store)?,
            make_tree(&keys[60..100], &store)?,
            make_tree(&keys[30..70], &store)?,
        ];

        let merged = HtreeNode::merge_many(trees, &store)?;
        let merged_keys = collect_keys_many(&merged, &store)?;

        assert_eq!(merged_keys, keys);

        // Verify iteration is in sorted order
        let mut prev: Option<UUID> = None;
        for k in &merged_keys {
            if let Some(p) = prev {
                assert!(p < *k, "keys not sorted: {p:?} >= {k:?}");
            }
            prev = Some(*k);
        }
        Ok(())
    }

    // =========================================================================
    // SPLIT-MERGE ROUND-TRIP
    // =========================================================================

    #[test]
    fn split_merge_roundtrip_at_random_positions() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(50);
        let tree = make_tree(&keys, &store)?;

        // Split at various positions and verify merge restores
        for split_pos in [0, 1, 10, 25, 40, 49] {
            let (lt, gte) = tree.split_at(&keys[split_pos], &store)?;
            let parts: Vec<HtreeNode<u64>> = [lt, gte].into_iter().flatten().collect();
            let merged = HtreeNode::merge_many(parts, &store)?;
            let merged_keys = collect_keys_many(&merged, &store)?;

            assert_eq!(merged_keys, keys, "failed at split position {split_pos}");
        }
        Ok(())
    }

    #[test]
    fn multiple_split_merge_cycles() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(40);
        let tree = make_tree(&keys, &store)?;

        // Split, merge, split again, merge again
        let (lt, gte) = tree.split_at(&keys[20], &store)?;
        let lt = lt.unwrap_or_default();
        let gte = gte.unwrap_or_default();

        let merged1 = lt.merge(gte, &store)?;

        let (lt2, gte2) = merged1.split_at(&keys[10], &store)?;
        let lt2 = lt2.unwrap_or_default();
        let gte2 = gte2.unwrap_or_default();

        let merged2 = lt2.merge(gte2, &store)?;

        let merged_keys = collect_keys(&merged2, &store)?;
        assert_eq!(merged_keys, keys);
        Ok(())
    }

    // =========================================================================
    // COMPLEXITY TESTS
    // =========================================================================

    /// Helper to build tree on a CountingStore
    fn make_tree_counting(
        keys: &[UUID],
        store: &CountingStore,
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

    /// Collect keys from tree with CountingStore
    fn collect_keys_counting(
        tree: &HtreeNode<u64>,
        store: &CountingStore,
    ) -> Result<Vec<UUID>, Box<dyn std::error::Error>> {
        Ok(tree.iter_keys(store).collect::<Result<Vec<_>, _>>()?)
    }

    /// Collect keys from multiple trees with CountingStore
    fn collect_keys_many_counting(
        trees: &[HtreeNode<u64>],
        store: &CountingStore,
    ) -> Result<Vec<UUID>, Box<dyn std::error::Error>> {
        let mut keys = Vec::new();
        for tree in trees {
            keys.extend(tree.iter_keys(store).collect::<Result<Vec<_>, _>>()?);
        }
        keys.sort();
        Ok(keys)
    }

    #[test]
    fn merge_complexity_scales_with_trees_not_leaves() -> Result<(), Box<dyn std::error::Error>> {
        // Compare: merge 2 trees of 50 leaves each vs 50 single-leaf merges
        // The former should use significantly fewer store ops
        let store = CountingStore::new();
        let keys = gen_keys(100);

        // Build two trees of 50 leaves each
        let tree_a = make_tree_counting(&keys[..50], &store)?;
        let tree_b = make_tree_counting(&keys[50..], &store)?;
        store.reset_counts();

        // Merge two trees at once
        let merged = tree_a.clone().merge(tree_b.clone(), &store)?;
        let ops_two_trees = store.total_ops();

        // Verify correctness
        let merged_keys = collect_keys_counting(&merged, &store)?;
        assert_eq!(merged_keys, keys);
        store.reset_counts();

        // Now simulate merging 50 single-leaf trees into a base tree
        let base = make_tree_counting(&keys[..50], &store)?;
        store.reset_counts();

        let mut result = base;
        for &key in &keys[50..] {
            let leaf = HtreeNode::<u64>::from_kvp(&key, &999, &store)?;
            result = result.merge(leaf, &store)?;
        }
        let ops_many_leaves = store.total_ops();

        // With batching optimization, merging 2 trees should be more efficient
        // than merging 50 leaves one by one
        // The batched approach should use fewer ops per leaf
        assert!(
            ops_two_trees < ops_many_leaves,
            "Merging 2 trees ({ops_two_trees} ops) should be cheaper than 50 sequential merges ({ops_many_leaves} ops)"
        );

        Ok(())
    }

    #[test]
    fn identical_subtrees_use_structural_sharing() -> Result<(), Box<dyn std::error::Error>> {
        let store = CountingStore::new();
        let keys = gen_keys(50);
        let tree = make_tree_counting(&keys, &store)?;

        store.reset_counts();

        // Merge tree with itself - should skip redundant work via seen set
        let merged = tree.clone().merge(tree.clone(), &store)?;
        let ops_self_merge = store.total_ops();

        // Verify correctness
        let merged_keys = collect_keys_counting(&merged, &store)?;
        assert_eq!(merged_keys, keys);

        // Self-merge should be very cheap since identical hkeys are detected
        // At minimum we need to fetch children of root to check hkeys
        // But we should NOT descend into subtrees that are identical
        let tree_height = tree.height() as usize;

        // Reasonable bound: should be much less than a full traversal would require
        // Full traversal would be O(N * H), we expect closer to O(H)
        let max_expected_ops = tree_height * 10; // generous upper bound
        assert!(
            ops_self_merge <= max_expected_ops,
            "Self-merge used {ops_self_merge} ops, expected <= {max_expected_ops} for height {tree_height}"
        );

        Ok(())
    }

    #[test]
    fn batched_leaves_more_efficient() -> Result<(), Box<dyn std::error::Error>> {
        // Insert multiple leaves into existing tree using merge_many vs sequential merge
        let store = CountingStore::new();
        let base_keys = gen_keys(40);
        let new_keys = gen_keys(20);

        // Build base tree
        let base = make_tree_counting(&base_keys, &store)?;
        store.reset_counts();

        // Method 1: merge all new leaves at once via merge_many
        let new_leaves: Vec<HtreeNode<u64>> = new_keys
            .iter()
            .enumerate()
            .map(|(i, k)| HtreeNode::<u64>::from_kvp(k, &(i as u64), &store))
            .collect::<Result<Vec<_>, _>>()?;
        store.reset_counts();

        let merged_batch = HtreeNode::merge_many(
            std::iter::once(base.clone()).chain(new_leaves.clone()),
            &store,
        )?;
        let ops_batched = store.total_ops();
        store.reset_counts();

        // Method 2: merge leaves one by one
        let mut merged_sequential = base.clone();
        for leaf in new_leaves {
            merged_sequential = merged_sequential.merge(leaf, &store)?;
        }
        let ops_sequential = store.total_ops();

        // Verify both produce same result
        let batch_keys = collect_keys_many_counting(&merged_batch, &store)?;
        store.reset_counts();
        let sequential_keys = collect_keys_counting(&merged_sequential, &store)?;

        let mut expected: Vec<UUID> = base_keys.clone();
        expected.extend(&new_keys);
        expected.sort();
        expected.dedup(); // In case of any overlapping keys

        assert_eq!(batch_keys, expected);
        assert_eq!(sequential_keys, expected);

        // Batched should be more efficient
        assert!(
            ops_batched < ops_sequential,
            "Batched merge ({ops_batched} ops) should be cheaper than sequential ({ops_sequential} ops)"
        );

        Ok(())
    }

    #[test]
    fn merge_many_trees_batches_efficiently() -> Result<(), Box<dyn std::error::Error>> {
        // Merging N trees should be much cheaper than merging them one by one
        let store = CountingStore::new();
        let all_keys = gen_keys(100);

        // Create 10 trees of 10 keys each
        let trees: Vec<HtreeNode<u64>> = (0..10)
            .map(|i| make_tree_counting(&all_keys[i * 10..(i + 1) * 10], &store))
            .collect::<Result<Vec<_>, _>>()?;
        store.reset_counts();

        // Method 1: merge all at once
        let merged_all = HtreeNode::merge_many(trees.clone(), &store)?;
        let ops_all_at_once = store.total_ops();
        store.reset_counts();

        // Method 2: merge sequentially (fold)
        let mut merged_seq = trees[0].clone();
        for tree in &trees[1..] {
            merged_seq = merged_seq.merge(tree.clone(), &store)?;
        }
        let ops_sequential = store.total_ops();

        // Verify correctness
        let all_at_once_keys = collect_keys_many_counting(&merged_all, &store)?;
        store.reset_counts();
        let seq_keys = collect_keys_counting(&merged_seq, &store)?;

        assert_eq!(all_at_once_keys, all_keys);
        assert_eq!(seq_keys, all_keys);

        // merge_many should be more efficient due to batching
        assert!(
            ops_all_at_once <= ops_sequential,
            "merge_many ({ops_all_at_once} ops) should be <= sequential ({ops_sequential} ops)"
        );

        Ok(())
    }
}
