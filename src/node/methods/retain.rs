use ps_hkey::Store;
use ps_uuid::UUID;

use crate::{HtreeNode, HtreeValue};

impl<T: HtreeValue> HtreeNode<T> {
    /// Keeps only entries where the predicate returns true, removing all others.
    ///
    /// # Errors
    ///
    /// Returns an error if store operations or value unpacking fails.
    ///
    /// # Examples
    ///
    /// ```
    /// use ps_htree::HtreeNode;
    /// use ps_hkey::InMemoryStore;
    /// use ps_uuid::UUID;
    ///
    /// let store = InMemoryStore::default();
    ///
    /// // Create a tree with values 0..10
    /// let leaves: Vec<_> = (0..10u64)
    ///     .map(|i| {
    ///         let key = UUID::gen_v4();
    ///         HtreeNode::from_kvp(&key, &i, &store).unwrap()
    ///     })
    ///     .collect();
    ///
    /// let tree = HtreeNode::from_many_children(leaves, &store)
    ///     .unwrap()
    ///     .into_iter()
    ///     .next()
    ///     .unwrap();
    ///
    /// // Keep only even values
    /// let filtered = tree.retain(|_key, value| value % 2 == 0, &store).unwrap();
    ///
    /// // Verify only 5 entries remain (0, 2, 4, 6, 8)
    /// let count = filtered.iter_entries(&store).count();
    /// assert_eq!(count, 5);
    /// ```
    pub fn retain<S, F>(
        &self,
        mut predicate: F,
        store: &S,
    ) -> Result<Self, HtreeNodeRetainError<T, S>>
    where
        S: Store,
        F: FnMut(UUID, T) -> bool,
    {
        Ok(self
            .retain_inner(&mut predicate, store)?
            .unwrap_or_default())
    }

    fn retain_inner<S, F>(
        &self,
        predicate: &mut F,
        store: &S,
    ) -> Result<Option<Self>, HtreeNodeRetainError<T, S>>
    where
        S: Store,
        F: FnMut(UUID, T) -> bool,
    {
        if self.is_empty() {
            return Ok(None);
        }

        if self.is_leaf() {
            let bytes = self
                .hkey
                .resolve(store)
                .map_err(HtreeNodeRetainError::Store)?;

            let value = T::unpack_from_bytes(bytes, store)?;

            return if predicate(self.key, value) {
                Ok(Some(self.clone()))
            } else {
                Ok(None)
            };
        }

        let original_children = self.fetch_children_guard(store)?;

        let children = original_children
            .iter()
            .filter_map(|child| child.retain_inner(predicate, store).transpose())
            .collect::<Result<Vec<Self>, _>>()?;

        if children.is_empty() {
            return Ok(None);
        }

        if *children == *original_children {
            return Ok(Some(self.clone()));
        }

        drop(original_children);

        Ok(Some(Self::from_children(children, store)?))
    }
}

/// Error type for retain operations.
#[derive(thiserror::Error, Debug)]
pub enum HtreeNodeRetainError<T: HtreeValue, S: Store> {
    /// Node state is invalid or corrupted.
    #[error("HtreeNode's state is corrupted.")]
    CorruptedState,

    /// Failed to reconstruct node after filtering.
    #[error("Node reconstruction failed: {0}")]
    FromChildren(crate::HtreeNodeFromChildrenError<S>),

    /// Underlying store operation failed.
    #[error("Store error: {0}")]
    Store(S::Error),

    /// Failed to unpack a value from stored bytes.
    #[error("Unpack error: {0}")]
    Unpack(T::UnpackError),

    /// Failed to deserialize child nodes.
    #[error("Error unpacking children: {0}")]
    UnpackChildren(#[from] crate::HtreeNodeUnpackChildrenError),
}

impl<T: HtreeValue, S: Store> From<crate::HtreeNodeFetchChildrenError<S>>
    for HtreeNodeRetainError<T, S>
{
    fn from(value: crate::HtreeNodeFetchChildrenError<S>) -> Self {
        match value {
            crate::HtreeNodeFetchChildrenError::CorruptedState => Self::CorruptedState,
            crate::HtreeNodeFetchChildrenError::Store(err) => Self::Store(err),
            crate::HtreeNodeFetchChildrenError::UnpackChildren(err) => Self::UnpackChildren(err),
        }
    }
}

impl<T: HtreeValue, S: Store> From<crate::HtreeNodeFromChildrenError<S>>
    for HtreeNodeRetainError<T, S>
{
    fn from(value: crate::HtreeNodeFromChildrenError<S>) -> Self {
        match value {
            crate::HtreeNodeFromChildrenError::Store(err) => Self::Store(err),
            err => Self::FromChildren(err),
        }
    }
}

impl<T: HtreeValue, S: Store> From<crate::HtreeValueUnpackError<T, S>>
    for HtreeNodeRetainError<T, S>
{
    fn from(value: crate::HtreeValueUnpackError<T, S>) -> Self {
        match value {
            crate::HtreeValueUnpackError::Store(err) => Self::Store(err),
            crate::HtreeValueUnpackError::Unpack(err) => Self::Unpack(err),
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use ps_hkey::InMemoryStore;
    use ps_uuid::UUID;

    use crate::HtreeNode;

    fn make_tree(store: &InMemoryStore, count: usize) -> (Vec<UUID>, HtreeNode<u64>) {
        let leaves: Vec<_> = (0..count)
            .map(|i| {
                let key = UUID::gen_v4();
                (
                    key,
                    HtreeNode::from_kvp(&key, &(i as u64), store)
                        .expect("from_kvp should create leaf node"),
                )
            })
            .collect();

        let keys: Vec<_> = leaves.iter().map(|(k, _)| *k).collect();
        let nodes: Vec<_> = leaves.into_iter().map(|(_, n)| n).collect();

        let mut roots = HtreeNode::from_many_children(nodes, store)
            .expect("from_many_children should build tree");
        while roots.len() > 1 {
            roots = HtreeNode::from_many_children(roots, store)
                .expect("from_many_children should succeed");
        }

        let tree = roots.into_iter().next().unwrap_or_default();
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

    #[test]
    fn empty_tree_returns_empty() {
        let store = InMemoryStore::default();
        let tree: HtreeNode<u64> = HtreeNode::default();

        let result = tree
            .retain(|_, _| true, &store)
            .expect("retain should succeed");

        assert!(result.is_empty());
    }

    #[test]
    fn retain_all_returns_equivalent_tree() {
        let store = InMemoryStore::default();
        let (keys, tree) = make_tree(&store, 10);

        let result = tree
            .retain(|_, _| true, &store)
            .expect("retain should succeed");

        // All keys should still be present
        for key in &keys {
            assert!(
                result.contains_key(key, &store).expect("contains_key"),
                "key should be present"
            );
        }
    }

    #[test]
    fn retain_none_returns_empty() {
        let store = InMemoryStore::default();
        let (_, tree) = make_tree(&store, 10);

        let result = tree
            .retain(|_, _| false, &store)
            .expect("retain should succeed");

        assert!(result.is_empty());
    }

    #[test]
    fn retain_filters_by_value() {
        let store = InMemoryStore::default();
        let (_, tree) = make_tree(&store, 20);

        // Keep only even values
        let result = tree
            .retain(|_, value| value % 2 == 0, &store)
            .expect("retain should succeed");

        let entries: Vec<_> = result
            .iter_entries(&store)
            .map(|r| r.expect("iter"))
            .collect();

        assert_eq!(entries.len(), 10);
        for (_, value) in &entries {
            assert_eq!(value % 2, 0);
        }
    }

    #[test]
    fn retain_filters_by_key() {
        let store = InMemoryStore::default();
        let (keys, tree) = make_tree(&store, 10);

        let keys_to_keep: std::collections::HashSet<_> = keys.iter().take(5).copied().collect();

        let result = tree
            .retain(|key, _| keys_to_keep.contains(&key), &store)
            .expect("retain should succeed");

        let entries: Vec<_> = result
            .iter_entries(&store)
            .map(|r| r.expect("iter"))
            .collect();

        assert_eq!(entries.len(), 5);
        for (key, _) in &entries {
            assert!(keys_to_keep.contains(key));
        }
    }

    #[test]
    fn retain_single_leaf_matching() {
        let store = InMemoryStore::default();
        let key = UUID::gen_v4();

        let tree = HtreeNode::from_kvp(&key, &42_u64, &store).expect("from_kvp should create leaf");

        let result = tree
            .retain(|_, v| v == 42, &store)
            .expect("retain should succeed");

        assert!(!result.is_empty());
        assert!(result.contains_key(&key, &store).expect("contains_key"));
    }

    #[test]
    fn retain_single_leaf_not_matching() {
        let store = InMemoryStore::default();
        let key = UUID::gen_v4();

        let tree = HtreeNode::from_kvp(&key, &42_u64, &store).expect("from_kvp should create leaf");

        let result = tree
            .retain(|_, v| v != 42, &store)
            .expect("retain should succeed");

        assert!(result.is_empty());
    }

    #[test]
    fn retain_preserves_sorted_order() {
        let store = InMemoryStore::default();
        let (_, tree) = make_tree(&store, 30);

        let result = tree
            .retain(|_, value| value % 3 == 0, &store)
            .expect("retain should succeed");

        let keys: Vec<_> = result.iter_keys(&store).map(|r| r.expect("iter")).collect();

        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted, "keys should be in sorted order");
    }

    #[test]
    fn retain_large_tree() {
        let store = InMemoryStore::default();
        let (_, tree) = make_tree(&store, 100);

        // Keep values in range [25, 75)
        let result = tree
            .retain(|_, value| (25..75).contains(&value), &store)
            .expect("retain should succeed");

        let entries: Vec<_> = result
            .iter_entries(&store)
            .map(|r| r.expect("iter"))
            .collect();

        assert_eq!(entries.len(), 50);
        for (_, value) in &entries {
            assert!(*value >= 25 && *value < 75);
        }
    }

    #[test]
    fn retain_structural_sharing_all_kept() {
        let store = InMemoryStore::default();
        let (_, tree) = make_tree(&store, 10);

        let original_hkey = tree.hkey.clone();

        let result = tree
            .retain(|_, _| true, &store)
            .expect("retain should succeed");

        // When all entries are kept, the root should be reused
        assert_eq!(result.hkey, original_hkey, "root hkey should be unchanged");
    }

    #[test]
    fn retain_complex_predicate() {
        let store = InMemoryStore::default();
        let (_, tree) = make_tree(&store, 30);

        let is_prime = |n: u64| -> bool {
            if n < 2 {
                return false;
            }
            for i in 2..=n.isqrt() {
                if n.is_multiple_of(i) {
                    return false;
                }
            }
            true
        };

        let result = tree
            .retain(|_, value| is_prime(value) || value % 10 == 0, &store)
            .expect("retain should succeed");

        let entries: Vec<_> = result
            .iter_entries(&store)
            .map(|r| r.expect("iter"))
            .collect();

        // Expected: 0, 2, 3, 5, 7, 10, 11, 13, 17, 19, 20, 23, 29 = 13 values
        assert_eq!(entries.len(), 13);
        for (_, value) in &entries {
            assert!(
                is_prime(*value) || *value % 10 == 0,
                "value {value} should match predicate"
            );
        }
    }

    #[test]
    fn retain_stress_alternating_pattern() {
        let store = InMemoryStore::default();
        let (_, tree) = make_tree(&store, 200);

        // Keep every other entry
        let result = tree
            .retain(|_, value| value % 2 == 0, &store)
            .expect("retain should succeed");

        let count = result
            .count_leaves(&store)
            .expect("count_leaves should succeed");

        assert_eq!(count, 100);
    }

    #[test]
    fn retain_keeps_one_from_each_subtree() {
        let store = InMemoryStore::default();
        let (_, tree) = make_tree(&store, 50);

        // Keep only values divisible by 7 (should be: 0, 7, 14, 21, 28, 35, 42, 49 = 8)
        let result = tree
            .retain(|_, value| value % 7 == 0, &store)
            .expect("retain should succeed");

        let count = result
            .iter_entries(&store)
            .try_fold(0, |acc, r| r.map(|_| acc + 1))
            .expect("iter_entries should succeed");

        assert_eq!(count, 8);
    }

    #[test]
    fn retain_prunes_entire_deep_wrapper_chain_when_leaf_removed() {
        let store = InMemoryStore::default();
        let leaf = HtreeNode::from_kvp("only", &(), &store).expect("create leaf");

        let mut tree = leaf;
        for _ in 0..12 {
            tree = HtreeNode::from_children([tree], &store).expect("wrap tree");
        }

        let result = tree
            .retain(|_, ()| false, &store)
            .expect("retain should prune deep wrapper chain");

        assert!(result.is_empty());
        assert_eq!(
            result
                .count_leaves(&store)
                .expect("count_leaves should work on empty tree"),
            0
        );
        assert!(result.iter_leaves(&store).next().is_none());
    }

    #[test]
    fn retain_prunes_sparse_branch_and_keeps_dense_branch_stable() {
        let store = InMemoryStore::default();

        let sparse_leaf = HtreeNode::from_kvp("sparse", &(), &store).expect("create sparse leaf");
        let sparse_key = sparse_leaf.key;
        let sparse_branch =
            HtreeNode::from_children([sparse_leaf], &store).expect("create sparse branch");
        let sparse_branch =
            HtreeNode::from_children([sparse_branch], &store).expect("wrap sparse branch");

        let dense_leaves: Vec<_> = (0..256)
            .map(|n| HtreeNode::from_kvp(&n, &(), &store).expect("create dense leaf"))
            .collect();
        let dense_keys: Vec<_> = dense_leaves.iter().map(|leaf| leaf.key).collect();

        let dense_packed = HtreeNode::from_many_children(dense_leaves, &store)
            .expect("build dense packed subtree");
        let dense_branch =
            HtreeNode::from_children(dense_packed, &store).expect("wrap dense branch");

        let root = HtreeNode::from_children([sparse_branch, dense_branch], &store)
            .expect("combine sparse and dense branches");

        let result = root
            .retain(|key, ()| key != sparse_key, &store)
            .expect("retain should prune sparse branch");

        assert!(
            !result
                .contains_key(&sparse_key, &store)
                .expect("contains_key")
        );
        assert_eq!(
            result
                .count_leaves(&store)
                .expect("count_leaves should succeed"),
            dense_keys.len()
        );

        for key in dense_keys.iter().take(8) {
            assert!(
                result.contains_key(key, &store).expect("contains_key"),
                "dense key should remain after pruning sparse branch"
            );
        }
    }

    #[test]
    fn retain_sequential_sparse_pruning_remains_traversable() {
        let store = InMemoryStore::default();

        let a = HtreeNode::from_kvp("a", &(), &store).expect("create leaf");
        let b = HtreeNode::from_kvp("b", &(), &store).expect("create leaf");
        let c = HtreeNode::from_kvp("c", &(), &store).expect("create leaf");
        let ak = a.key;
        let bk = b.key;
        let ck = c.key;

        let group_a = HtreeNode::from_children([a], &store).expect("group single leaf");
        let group_b = HtreeNode::from_children([b, c], &store).expect("group two leaves");

        let dense_count = 300usize;
        let packed = HtreeNode::from_many_children(
            (10_000..10_000 + dense_count as u64)
                .map(|n| HtreeNode::from_kvp(&n, &(), &store).expect("create dense leaf")),
            &store,
        )
        .expect("build dense subtree");

        let root = HtreeNode::from_children(
            [
                HtreeNode::from_children([group_a], &store).expect("wrap sparse group"),
                HtreeNode::from_children([group_b], &store).expect("wrap sparse group"),
                HtreeNode::from_children(packed, &store).expect("wrap dense subtree"),
            ],
            &store,
        )
        .expect("combine branches");

        let ret1 = root
            .retain(|_, ()| true, &store)
            .expect("retain should keep all");
        assert_eq!(ret1.count_leaves(&store).expect("count"), dense_count + 3);

        let ret2 = ret1
            .retain(|key, ()| key != bk, &store)
            .expect("retain should prune one sparse leaf");
        assert_eq!(ret2.count_leaves(&store).expect("count"), dense_count + 2);

        let ret3 = ret2
            .retain(|key, ()| key != ck, &store)
            .expect("retain should prune second sparse leaf");
        assert_eq!(ret3.count_leaves(&store).expect("count"), dense_count + 1);

        let ret4 = ret3
            .retain(|key, ()| key != ak, &store)
            .expect("retain should prune final sparse leaf");
        assert_eq!(ret4.count_leaves(&store).expect("count"), dense_count);

        let keys: Vec<_> = ret4
            .iter_keys(&store)
            .map(|r| r.expect("iter_keys"))
            .collect();
        assert_eq!(keys.len(), dense_count);
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted, "resulting dense tree should stay sorted");
    }

    #[test]
    fn retain_to_empty_is_idempotent_across_repeated_calls() {
        let store = InMemoryStore::default();
        let (_, tree) = make_tree(&store, 128);

        let filtered = tree
            .retain(|_, value| value % 2 == 0, &store)
            .expect("retain should succeed");

        let mut empty = filtered
            .retain(|_, _| false, &store)
            .expect("retain should produce empty tree");

        for _ in 0..5 {
            assert!(empty.is_empty());
            assert_eq!(empty.count_leaves(&store).expect("count"), 0);
            assert!(empty.iter_entries(&store).next().is_none());

            empty = empty
                .retain(|_, _| true, &store)
                .expect("retain on empty tree should be stable");
        }
    }

    #[test]
    fn retain_removing_duplicate_key_run_keeps_tree_valid() {
        let store = InMemoryStore::default();
        let duplicate_key = UUID::gen_v4();

        let duplicate_nodes = (0_u64..40).map(|i| {
            HtreeNode::from_kvp(&duplicate_key, &i, &store).expect("create duplicate-key leaf")
        });

        let unique_nodes = (0_u64..30).map(|i| {
            let key = UUID::gen_v4();
            HtreeNode::from_kvp(&key, &(100 + i), &store).expect("create unique-key leaf")
        });

        let root = collapse_to_root(
            HtreeNode::from_many_children(duplicate_nodes.chain(unique_nodes), &store)
                .expect("build mixed tree"),
            &store,
        );

        let result = root
            .retain(|key, _| key != duplicate_key, &store)
            .expect("retain should remove duplicate-key run");

        assert_eq!(result.count_leaves(&store).expect("count"), 30);
        assert!(
            !result
                .contains_key(&duplicate_key, &store)
                .expect("contains_key")
        );
        assert!(
            result
                .iter_entries(&store)
                .map(|r| r.expect("iter"))
                .all(|(key, _)| key != duplicate_key)
        );
    }

    #[test]
    fn retain_single_survivor_remains_queryable() {
        let store = InMemoryStore::default();
        let (keys, tree) = make_tree(&store, 300);
        let survivor_index = 157usize;
        let survivor_key = keys[survivor_index];

        let result = tree
            .retain(|key, _| key == survivor_key, &store)
            .expect("retain should keep one key");

        assert_eq!(result.count_leaves(&store).expect("count"), 1);
        assert!(
            result
                .contains_key(&survivor_key, &store)
                .expect("contains_key")
        );

        let first = result.first(&store).expect("first should succeed");
        let last = result.last(&store).expect("last should succeed");
        assert_eq!(
            first.expect("single survivor should have first").key,
            survivor_key
        );
        assert_eq!(
            last.expect("single survivor should have last").key,
            survivor_key
        );

        let value = result
            .find_one_value(&survivor_key, &store)
            .expect("find_one_value should succeed")
            .expect("survivor key should exist");
        assert_eq!(value, survivor_index as u64);
    }

    #[test]
    fn retain_matches_delete_one_for_key_removal_set() {
        let store = InMemoryStore::default();
        let (keys, tree) = make_tree(&store, 90);
        let keys_to_drop: Vec<_> = keys.iter().take(30).copied().collect();
        let keys_to_drop_set: std::collections::HashSet<_> = keys_to_drop.iter().copied().collect();

        let retained = tree
            .retain(|key, _| !keys_to_drop_set.contains(&key), &store)
            .expect("retain should succeed");

        let deleted = keys_to_drop.iter().fold(tree, |acc, key| {
            acc.delete_one(key, &store)
                .expect("delete_one should succeed")
        });

        let retained_keys: Vec<_> = retained
            .iter_keys(&store)
            .map(|r| r.expect("iter_keys should succeed"))
            .collect();
        let deleted_keys: Vec<_> = deleted
            .iter_keys(&store)
            .map(|r| r.expect("iter_keys should succeed"))
            .collect();

        assert_eq!(retained_keys, deleted_keys);
        assert_eq!(
            retained.count_leaves(&store).expect("count"),
            deleted.count_leaves(&store).expect("count")
        );
    }

    #[test]
    fn retain_with_multiple_predicate_rounds_preserves_order_and_validity() {
        let store = InMemoryStore::default();
        let (_, tree) = make_tree(&store, 240);

        let mut current = tree;
        let mut predicates_applied: Vec<u64> = Vec::new();

        for divisor in [2_u64, 3, 5, 7, 11] {
            predicates_applied.push(divisor);
            current = current
                .retain(
                    |_, value| !predicates_applied.iter().any(|d| value % d == 0),
                    &store,
                )
                .expect("retain should succeed");

            let entries: Vec<_> = current
                .iter_entries(&store)
                .map(|r| r.expect("iter_entries should succeed"))
                .collect();

            let keys: Vec<_> = entries.iter().map(|(k, _)| *k).collect();
            let mut sorted = keys.clone();
            sorted.sort();
            assert_eq!(keys, sorted, "keys should remain sorted after each retain");

            assert!(entries.iter().all(|(_, value)| {
                !predicates_applied
                    .iter()
                    .any(|divisor| value % divisor == 0)
            }));
        }
    }

    #[test]
    fn various_density() {
        let store = InMemoryStore::default();

        let a = HtreeNode::from_kvp("a", &(), &store).expect("create leaf from string key");
        let b = HtreeNode::from_kvp("b", &(), &store).expect("create leaf from string key");
        let c = HtreeNode::from_kvp("c", &(), &store).expect("create leaf from string key");

        let ak = a.key;
        let bk = b.key;
        let ck = c.key;

        let group_a = HtreeNode::from_children([a], &store).expect("group single leaf");
        let group_b = HtreeNode::from_children([b, c], &store).expect("group two leaves");

        let packed = HtreeNode::from_many_children(
            (1000..2000)
                .map(|n| HtreeNode::from_kvp(&n, &(), &store).expect("create leaf from int key")),
            &store,
        )
        .expect("build dense subtree from 1000 leaves");

        let node = HtreeNode::from_children(
            [
                HtreeNode::from_children([group_a], &store).expect("wrap sparse group"),
                HtreeNode::from_children([group_b], &store).expect("wrap sparse group"),
                HtreeNode::from_children(packed, &store).expect("wrap dense subtree"),
            ],
            &store,
        )
        .expect("combine sparse and dense subtrees");
        let ret1 = node
            .retain(|_, ()| true, &store)
            .expect("retain should handle mixed-density tree");
        let ret2 = ret1
            .retain(|key, ()| key != bk, &store)
            .expect("retain should handle mixed-density tree");
        let ret3 = ret2
            .retain(|key, ()| key != ck, &store)
            .expect("retain should handle an empty subtree");
        let ret4 = ret3
            .retain(|key, ()| key != ak, &store)
            .expect("retain should handle an empty subtree");
        let leaf_count = ret4
            .count_leaves(&store)
            .expect("counting leaves shouldn't fail");

        assert_eq!(
            &leaf_count, &1000,
            "Incorrect number of leaves after retain operations."
        );
    }
}
