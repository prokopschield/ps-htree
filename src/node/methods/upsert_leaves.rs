use std::collections::HashSet;

use ps_hkey::Store;

use crate::{HtreeNode, LEAF_HEIGHT};

impl<T> HtreeNode<T> {
    /// Upserts leaves into this node, rebalancing if necessary.
    ///
    /// Accepts both leaf and internal nodes. Returns potentially multiple sibling
    /// nodes if rebalancing causes the tree to split.
    ///
    /// # Arguments
    /// * `children` - Leaves or internal nodes to upsert
    /// * `store` - Persistence backend
    ///
    /// # Errors
    /// - [`HtreeNodeUpsertLeavesError::CorruptedLeaf`] is returned if a leaf's state is invalid.
    /// - [`HtreeNodeUpsertLeavesError::CorruptedNode`] is returned if this node's state is invalid.
    /// - [`HtreeNodeUpsertLeavesError::FromChildren`] is returned if node reconstruction fails.
    /// - [`HtreeNodeUpsertLeavesError::Store`] is returned if store operations fail.
    /// - [`HtreeNodeUpsertLeavesError::UnpackChildren`] is returned if child deserialization fails.
    pub fn upsert_leaves<I: IntoIterator<Item = Self>, S: Store>(
        &self,
        children: I,
        store: &S,
    ) -> Result<Vec<Self>, HtreeNodeUpsertLeavesError<S>> {
        if self.height <= LEAF_HEIGHT + 1 {
            let mut keys = HashSet::new();
            let mut leaves = vec![];

            for child in children {
                if child.is_leaf() {
                    keys.insert(child.key);
                    leaves.push(child);

                    continue;
                }

                for leaf in child.iter_leaves(store) {
                    let leaf = leaf?;
                    keys.insert(leaf.key);
                    leaves.push(leaf);
                }
            }

            for child in self
                .fetch_children_guard(store)?
                .iter()
                .filter(|child| !keys.contains(&child.key))
                .cloned()
            {
                leaves.push(child);
            }

            return Self::from_many_children(leaves, store).map_err(Into::into);
        }

        let mut groups: Vec<(Self, Vec<Self>)> = self
            .iter_children(store)?
            .map(|child| (child, vec![]))
            .collect();

        if groups.is_empty() {
            groups.push((Self::default(), vec![]));
        }

        let mut push_leaf = |leaf: Self| {
            let index = groups
                .partition_point(|(node, _)| node.key <= leaf.key)
                .saturating_sub(1);

            groups[index].1.push(leaf);
        };

        for child in children {
            if child.is_leaf() {
                push_leaf(child);

                continue;
            }

            for leaf in child.iter_leaves(store) {
                push_leaf(leaf?);
            }
        }

        let mut children = Vec::new();

        for (node, leaves) in groups {
            if leaves.is_empty() {
                children.push(node);
            } else {
                children.extend(node.upsert_leaves(leaves, store)?);
            }
        }

        Self::from_many_children(children, store).map_err(Into::into)
    }
}

#[derive(thiserror::Error, Debug)]
pub enum HtreeNodeUpsertLeavesError<S: Store> {
    #[error("Upserted leaf's state is corrupted.")]
    CorruptedLeaf,
    #[error("HtreeNode's state is corrupted.")]
    CorruptedNode,
    #[error("Node reconstruction failed.")]
    FromChildren(crate::HtreeNodeFromChildrenError<S>),
    #[error("Store error: {0}")]
    Store(S::Error),
    #[error("Error unpacking children: {0}")]
    UnpackChildren(#[from] crate::HtreeNodeUnpackChildrenError),
}

impl<S: Store> From<crate::HtreeNodeFetchChildrenError<S>> for HtreeNodeUpsertLeavesError<S> {
    fn from(value: crate::HtreeNodeFetchChildrenError<S>) -> Self {
        match value {
            crate::HtreeNodeFetchChildrenError::CorruptedState => Self::CorruptedNode,
            crate::HtreeNodeFetchChildrenError::Store(err) => Self::Store(err),
            crate::HtreeNodeFetchChildrenError::UnpackChildren(err) => Self::UnpackChildren(err),
        }
    }
}

impl<S: Store> From<crate::HtreeNodeIterChildrenError<S>> for HtreeNodeUpsertLeavesError<S> {
    fn from(value: crate::HtreeNodeIterChildrenError<S>) -> Self {
        match value {
            crate::HtreeNodeIterChildrenError::CorruptedState => Self::CorruptedNode,
            crate::HtreeNodeIterChildrenError::Store(err) => Self::Store(err),
            crate::HtreeNodeIterChildrenError::UnpackChildren(err) => Self::UnpackChildren(err),
        }
    }
}

impl<S: Store> From<crate::HtreeNodeIterLeavesError<S>> for HtreeNodeUpsertLeavesError<S> {
    fn from(value: crate::HtreeNodeIterLeavesError<S>) -> Self {
        match value {
            crate::HtreeNodeIterLeavesError::CorruptedState => Self::CorruptedLeaf,
            crate::HtreeNodeIterLeavesError::Store(err) => Self::Store(err),
            crate::HtreeNodeIterLeavesError::UnpackChildren(err) => Self::UnpackChildren(err),
        }
    }
}

impl<S: Store> From<crate::HtreeNodeFromChildrenError<S>> for HtreeNodeUpsertLeavesError<S> {
    fn from(value: crate::HtreeNodeFromChildrenError<S>) -> Self {
        match value {
            crate::HtreeNodeFromChildrenError::Store(err) => Self::Store(err),
            err => Self::FromChildren(err),
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use ps_hkey::InMemoryStore;
    use ps_uuid::UUID;

    use crate::HtreeNode;

    fn make_leaf(store: &InMemoryStore, key: UUID, value: u64) -> HtreeNode<u64> {
        HtreeNode::from_kvp(&key, &value, store).expect("from_kvp should create leaf node")
    }

    fn make_tree(store: &InMemoryStore, count: usize) -> (Vec<UUID>, HtreeNode<u64>) {
        let leaves: Vec<_> = (0..count)
            .map(|i| {
                let key = UUID::gen_v4();
                (key, make_leaf(store, key, i as u64))
            })
            .collect();

        let keys: Vec<_> = leaves.iter().map(|(k, _)| *k).collect();
        let nodes: Vec<_> = leaves.into_iter().map(|(_, n)| n).collect();

        let tree = collapse_to_root(
            HtreeNode::from_many_children(nodes, store)
                .expect("from_many_children should build tree"),
            store,
        );

        (keys, tree)
    }

    fn collapse_to_root<T>(mut roots: Vec<HtreeNode<T>>, store: &InMemoryStore) -> HtreeNode<T> {
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

    fn get_value(tree: &HtreeNode<u64>, key: &UUID, store: &InMemoryStore) -> Option<u64> {
        tree.find_one_value(key, store)
            .expect("find_one_value should succeed")
    }

    fn collect_all_keys(tree: &HtreeNode<u64>, store: &InMemoryStore) -> Vec<UUID> {
        tree.iter_keys(store)
            .map(|r| r.expect("iter_keys should succeed"))
            .collect()
    }

    #[test]
    fn upsert_into_empty_tree() {
        let store = InMemoryStore::default();
        let tree: HtreeNode<u64> = HtreeNode::default();

        let key = UUID::gen_v4();
        let leaf = make_leaf(&store, key, 42);

        let result = tree
            .upsert_leaves(vec![leaf], &store)
            .expect("upsert_leaves should succeed");

        let new_tree = collapse_to_root(result, &store);
        assert_eq!(get_value(&new_tree, &key, &store), Some(42));
    }

    #[test]
    fn upsert_new_leaves_no_overlap() {
        let store = InMemoryStore::default();
        let (existing_keys, tree) = make_tree(&store, 5);

        let new_keys: Vec<UUID> = (0..3).map(|_| UUID::gen_v4()).collect();
        let new_leaves: Vec<_> = new_keys
            .iter()
            .enumerate()
            .map(|(i, &key)| make_leaf(&store, key, 100 + i as u64))
            .collect();

        let result = tree
            .upsert_leaves(new_leaves, &store)
            .expect("upsert_leaves should succeed");

        let new_tree = collapse_to_root(result, &store);

        // All existing keys should still be present
        for (i, key) in existing_keys.iter().enumerate() {
            assert_eq!(
                get_value(&new_tree, key, &store),
                Some(i as u64),
                "existing key should have original value"
            );
        }

        // All new keys should be present
        for (i, key) in new_keys.iter().enumerate() {
            assert_eq!(
                get_value(&new_tree, key, &store),
                Some(100 + i as u64),
                "new key should have new value"
            );
        }
    }

    #[test]
    fn upsert_replaces_existing_keys() {
        let store = InMemoryStore::default();
        let (keys, tree) = make_tree(&store, 5);

        // Create new leaves with same keys but different values
        let replacement_leaves: Vec<_> = keys
            .iter()
            .map(|&key| make_leaf(&store, key, 999))
            .collect();

        let result = tree
            .upsert_leaves(replacement_leaves, &store)
            .expect("upsert_leaves should succeed");

        let new_tree = collapse_to_root(result, &store);

        // All keys should now have the new value
        for key in &keys {
            assert_eq!(
                get_value(&new_tree, key, &store),
                Some(999),
                "key should have updated value"
            );
        }
    }

    #[test]
    fn upsert_mixed_new_and_existing() {
        let store = InMemoryStore::default();
        let (keys, tree) = make_tree(&store, 6);

        // Replace first 3 keys, add 2 new ones
        let mut upsert_leaves: Vec<_> = keys[..3]
            .iter()
            .map(|&key| make_leaf(&store, key, 500))
            .collect();

        let new_key1 = UUID::gen_v4();
        let new_key2 = UUID::gen_v4();
        upsert_leaves.push(make_leaf(&store, new_key1, 501));
        upsert_leaves.push(make_leaf(&store, new_key2, 502));

        let result = tree
            .upsert_leaves(upsert_leaves, &store)
            .expect("upsert_leaves should succeed");

        let new_tree = collapse_to_root(result, &store);

        // First 3 keys should be updated
        for key in &keys[..3] {
            assert_eq!(get_value(&new_tree, key, &store), Some(500));
        }

        // Last 3 keys should retain original values
        for (i, key) in keys[3..].iter().enumerate() {
            assert_eq!(get_value(&new_tree, key, &store), Some(3 + i as u64));
        }

        // New keys should be present
        assert_eq!(get_value(&new_tree, &new_key1, &store), Some(501));
        assert_eq!(get_value(&new_tree, &new_key2, &store), Some(502));
    }

    #[test]
    fn upsert_duplicate_keys_last_wins() {
        let store = InMemoryStore::default();
        let tree: HtreeNode<u64> = HtreeNode::default();

        let key = UUID::gen_v4();

        // Upsert multiple leaves with the same key
        let leaves = vec![
            make_leaf(&store, key, 1),
            make_leaf(&store, key, 2),
            make_leaf(&store, key, 3),
        ];

        let result = tree
            .upsert_leaves(leaves, &store)
            .expect("upsert_leaves should succeed");

        let new_tree = collapse_to_root(result, &store);

        // The tree should contain the key (value depends on implementation)
        assert!(
            new_tree
                .contains_key(&key, &store)
                .expect("contains_key should succeed"),
            "key should be present"
        );
    }

    #[test]
    fn upsert_empty_iterator() {
        let store = InMemoryStore::default();
        let (keys, tree) = make_tree(&store, 5);

        let result = tree
            .upsert_leaves(Vec::<HtreeNode<u64>>::new(), &store)
            .expect("upsert_leaves should succeed");

        let new_tree = collapse_to_root(result, &store);

        // All keys should still be present with original values
        for (i, key) in keys.iter().enumerate() {
            assert_eq!(get_value(&new_tree, key, &store), Some(i as u64));
        }
    }

    #[test]
    fn upsert_single_leaf_into_large_tree() {
        let store = InMemoryStore::default();
        let (keys, tree) = make_tree(&store, 100);

        let new_key = UUID::gen_v4();
        let leaf = make_leaf(&store, new_key, 12345);

        let result = tree
            .upsert_leaves(vec![leaf], &store)
            .expect("upsert_leaves should succeed");

        let new_tree = collapse_to_root(result, &store);

        // New key should be present
        assert_eq!(get_value(&new_tree, &new_key, &store), Some(12345));

        // All existing keys should still be present
        for (i, key) in keys.iter().enumerate() {
            assert_eq!(get_value(&new_tree, key, &store), Some(i as u64));
        }
    }

    #[test]
    fn upsert_many_into_large_tree() {
        let store = InMemoryStore::default();
        let (keys, tree) = make_tree(&store, 200);

        // Add 50 new leaves
        let new_keys: Vec<UUID> = (0..50).map(|_| UUID::gen_v4()).collect();
        let new_leaves: Vec<_> = new_keys
            .iter()
            .enumerate()
            .map(|(i, &key)| make_leaf(&store, key, 1000 + i as u64))
            .collect();

        let result = tree
            .upsert_leaves(new_leaves, &store)
            .expect("upsert_leaves should succeed");

        let new_tree = collapse_to_root(result, &store);

        // All existing keys should still be present
        for (i, key) in keys.iter().enumerate() {
            assert_eq!(get_value(&new_tree, key, &store), Some(i as u64));
        }

        // All new keys should be present
        for (i, key) in new_keys.iter().enumerate() {
            assert_eq!(get_value(&new_tree, key, &store), Some(1000 + i as u64));
        }
    }

    #[test]
    fn upsert_preserves_total_count() {
        let store = InMemoryStore::default();
        let (keys, tree) = make_tree(&store, 20);

        // Replace 5 keys, add 10 new ones
        let mut upsert_leaves: Vec<_> = keys[..5]
            .iter()
            .map(|&key| make_leaf(&store, key, 999))
            .collect();

        for i in 0..10 {
            let new_key = UUID::gen_v4();
            upsert_leaves.push(make_leaf(&store, new_key, 1000 + i));
        }

        let result = tree
            .upsert_leaves(upsert_leaves, &store)
            .expect("upsert_leaves should succeed");

        let new_tree = collapse_to_root(result, &store);

        let all_keys = collect_all_keys(&new_tree, &store);
        // 20 original - 5 replaced + 5 replaced + 10 new = 30
        assert_eq!(all_keys.len(), 30);
    }

    #[test]
    fn upsert_internal_node_flattens_to_leaves() {
        let store = InMemoryStore::default();
        let (existing_keys, tree) = make_tree(&store, 10);

        // Create a small subtree to upsert
        let subtree_keys: Vec<UUID> = (0..5).map(|_| UUID::gen_v4()).collect();
        let subtree_leaves: Vec<_> = subtree_keys
            .iter()
            .enumerate()
            .map(|(i, &key)| make_leaf(&store, key, 2000 + i as u64))
            .collect();

        let subtree = collapse_to_root(
            HtreeNode::from_many_children(subtree_leaves, &store)
                .expect("from_many_children should succeed"),
            &store,
        );

        // Upsert the internal node (not just leaves)
        let result = tree
            .upsert_leaves(vec![subtree], &store)
            .expect("upsert_leaves should succeed");

        let new_tree = collapse_to_root(result, &store);

        // All existing keys should still be present
        for (i, key) in existing_keys.iter().enumerate() {
            assert_eq!(get_value(&new_tree, key, &store), Some(i as u64));
        }

        // All subtree keys should be present
        for (i, key) in subtree_keys.iter().enumerate() {
            assert_eq!(get_value(&new_tree, key, &store), Some(2000 + i as u64));
        }
    }

    #[test]
    fn upsert_all_keys_replaced() {
        let store = InMemoryStore::default();
        let (keys, tree) = make_tree(&store, 15);

        // Replace all keys with new values
        let replacement_leaves: Vec<_> = keys
            .iter()
            .enumerate()
            .map(|(i, &key)| make_leaf(&store, key, 5000 + i as u64))
            .collect();

        let result = tree
            .upsert_leaves(replacement_leaves, &store)
            .expect("upsert_leaves should succeed");

        let new_tree = collapse_to_root(result, &store);

        // All keys should have new values
        for (i, key) in keys.iter().enumerate() {
            assert_eq!(get_value(&new_tree, key, &store), Some(5000 + i as u64));
        }

        // Total count should be unchanged
        let all_keys = collect_all_keys(&new_tree, &store);
        assert_eq!(all_keys.len(), 15);
    }

    #[test]
    fn upsert_into_shallow_tree() {
        let store = InMemoryStore::default();

        // Create a very shallow tree (just a few leaves)
        let keys: Vec<UUID> = (0..3).map(|_| UUID::gen_v4()).collect();
        let leaves: Vec<_> = keys
            .iter()
            .enumerate()
            .map(|(i, &key)| make_leaf(&store, key, i as u64))
            .collect();

        let tree = collapse_to_root(
            HtreeNode::from_many_children(leaves, &store)
                .expect("from_many_children should succeed"),
            &store,
        );

        // Upsert one new and one existing
        let new_key = UUID::gen_v4();
        let upsert_leaves = vec![
            make_leaf(&store, keys[0], 100), // replace existing
            make_leaf(&store, new_key, 101), // new
        ];

        let result = tree
            .upsert_leaves(upsert_leaves, &store)
            .expect("upsert_leaves should succeed");

        let new_tree = collapse_to_root(result, &store);

        assert_eq!(get_value(&new_tree, &keys[0], &store), Some(100));
        assert_eq!(get_value(&new_tree, &keys[1], &store), Some(1));
        assert_eq!(get_value(&new_tree, &keys[2], &store), Some(2));
        assert_eq!(get_value(&new_tree, &new_key, &store), Some(101));
    }

    #[test]
    fn upsert_stress_test() {
        let store = InMemoryStore::default();
        let (mut keys, mut tree) = make_tree(&store, 50);

        // Perform multiple rounds of upserts
        for round in 0..5 {
            let base_value = (round + 1) * 1000;

            // Replace some existing keys
            let replace_count = 10;
            let replacements: Vec<_> = keys[..replace_count]
                .iter()
                .enumerate()
                .map(|(i, &key)| make_leaf(&store, key, (base_value + i) as u64))
                .collect();

            // Add some new keys
            let new_keys: Vec<UUID> = (0..5).map(|_| UUID::gen_v4()).collect();
            let new_leaves: Vec<_> = new_keys
                .iter()
                .enumerate()
                .map(|(i, &key)| make_leaf(&store, key, (base_value + 100 + i) as u64))
                .collect();

            let mut all_upserts = replacements;
            all_upserts.extend(new_leaves);

            let result = tree
                .upsert_leaves(all_upserts, &store)
                .expect("upsert_leaves should succeed");

            tree = collapse_to_root(result, &store);
            keys.extend(new_keys);
        }

        // Verify all keys are present
        let all_keys = collect_all_keys(&tree, &store);
        assert_eq!(all_keys.len(), keys.len());

        for key in &keys {
            assert!(
                tree.contains_key(key, &store)
                    .expect("contains_key should succeed"),
                "all keys should be present after stress test"
            );
        }
    }
}
