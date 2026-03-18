use ps_hkey::Store;

use crate::HtreeNode;

/// Result of a pop operation.
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use]
pub struct PopResult<T> {
    /// The popped node, or `None` if the tree was empty.
    pub popped: Option<HtreeNode<T>>,
    /// The remaining tree after the pop, or `None` when the tree is empty.
    pub remaining: Option<HtreeNode<T>>,
}

impl<T> HtreeNode<T> {
    /// Removes and returns the leaf with the smallest key from the tree.
    ///
    /// This is analogous to [`BTreeMap::pop_first`](std::collections::BTreeMap::pop_first).
    /// It finds the minimum key in the tree, removes its entry, and returns both
    /// the modified tree and the removed leaf in a single pass.
    ///
    /// Takes ownership of `self` because the original tree is consumed and the
    /// caller should use the `remaining` tree from the result.
    ///
    /// # Arguments
    ///
    /// * `store` - The persistence backend used for tree traversal and reconstruction.
    ///
    /// # Returns
    ///
    /// Returns a [`PopResult`] with:
    /// - `popped`: The removed leaf, or `None` if the tree was empty
    /// - `remaining`: The tree after removal (`None` if nothing remains)
    ///
    /// # Errors
    ///
    /// - [`HtreeNodePopFirstError::CorruptedState`] if stored tree structure is invalid.
    /// - [`HtreeNodePopFirstError::FromChildren`] if tree reconstruction fails.
    /// - [`HtreeNodePopFirstError::Store`] if store operations fail.
    /// - [`HtreeNodePopFirstError::UnpackChildren`] if child payload decoding fails.
    ///
    /// # Examples
    ///
    /// ```
    /// use ps_hkey::InMemoryStore;
    /// use ps_uuid::UUID;
    /// use ps_htree::HtreeNode;
    ///
    /// let store = InMemoryStore::default();
    ///
    /// // Empty tree returns None
    /// let empty: HtreeNode<u64> = HtreeNode::default();
    /// let result = empty.pop_first(&store).unwrap();
    /// assert!(result.popped.is_none());
    /// assert!(result.remaining.is_none());
    ///
    /// // Tree with entries returns the minimum
    /// let key1 = UUID::gen_v4().with_version(8);
    /// let key2 = UUID::gen_v4().with_version(8);
    /// let min_key = std::cmp::min(key1, key2);
    ///
    /// let leaf1 = HtreeNode::<u64>::from_kvp(&key1, &1, &store).unwrap();
    /// let leaf2 = HtreeNode::<u64>::from_kvp(&key2, &2, &store).unwrap();
    /// let tree = HtreeNode::from_many_children([leaf1, leaf2], &store)
    ///     .unwrap()
    ///     .into_iter()
    ///     .next()
    ///     .unwrap();
    ///
    /// let result = tree.pop_first(&store).unwrap();
    /// let popped = result.popped.unwrap();
    /// let remaining = result.remaining.unwrap();
    /// assert_eq!(popped.key, min_key);
    /// assert!(remaining.find_one(&min_key, &store).unwrap().is_none());
    /// ```
    pub fn pop_first<S: Store>(self, store: &S) -> Result<PopResult<T>, HtreeNodePopFirstError<S>> {
        if self.is_empty() {
            return Ok(PopResult {
                popped: None,
                remaining: None,
            });
        }

        if self.is_leaf() {
            return Ok(PopResult {
                popped: Some(self),
                remaining: None,
            });
        }

        self.pop_first_internal(store)
    }

    fn pop_first_internal<S: Store>(
        self,
        store: &S,
    ) -> Result<PopResult<T>, HtreeNodePopFirstError<S>> {
        let mut iter = self.iter_children(store)?;

        while let Some(first_child) = iter.next() {
            let PopResult { popped, remaining } = first_child.pop_first(store)?;

            let Some(popped) = popped else {
                continue;
            };

            let remaining = Self::from_children(
                remaining
                    .into_iter()
                    .chain(iter)
                    .filter(|child| !child.is_empty()),
                store,
            )?;

            let remaining = (!remaining.is_empty()).then_some(remaining);

            return Ok(PopResult {
                popped: Some(popped),
                remaining,
            });
        }

        Ok(PopResult {
            popped: None,
            remaining: None,
        })
    }
}

#[derive(thiserror::Error, Debug)]
pub enum HtreeNodePopFirstError<S: Store> {
    #[error("HtreeNode's state is internally corrupted.")]
    CorruptedState,

    #[error(transparent)]
    FromChildren(crate::HtreeNodeFromChildrenError<S>),

    #[error("Store error: {0}")]
    Store(S::Error),

    #[error(transparent)]
    UnpackChildren(#[from] crate::HtreeNodeUnpackChildrenError),
}

impl<S: Store> From<crate::HtreeNodeIterChildrenError<S>> for HtreeNodePopFirstError<S> {
    fn from(value: crate::HtreeNodeIterChildrenError<S>) -> Self {
        match value {
            crate::HtreeNodeIterChildrenError::CorruptedState => Self::CorruptedState,
            crate::HtreeNodeIterChildrenError::Store(err) => Self::Store(err),
            crate::HtreeNodeIterChildrenError::UnpackChildren(err) => Self::UnpackChildren(err),
        }
    }
}

impl<S: Store> From<crate::HtreeNodeFromChildrenError<S>> for HtreeNodePopFirstError<S> {
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

    use crate::{HtreeNode, MAX_CHILDREN};

    #[test]
    fn empty_tree_returns_none() {
        let store = InMemoryStore::default();
        let tree: HtreeNode<u64> = HtreeNode::default();

        let result = tree
            .pop_first(&store)
            .expect("pop_first should not fail on empty tree");

        assert!(result.popped.is_none());
        assert!(result.remaining.is_none());
    }

    #[test]
    fn single_leaf_pops_and_returns_empty() {
        let store = InMemoryStore::default();
        let key = UUID::gen_v4().with_version(8);
        let value = 42_u64;

        let tree = HtreeNode::from_kvp(&key, &value, &store)
            .expect("from_kvp should create a valid leaf node");

        let result = tree
            .pop_first(&store)
            .expect("pop_first should succeed on single leaf tree");

        let popped = result.popped.expect("popped should be Some");
        assert_eq!(popped.key, key);
        assert!(popped.is_leaf());
        assert!(result.remaining.is_none());
    }

    #[test]
    fn two_leaves_pops_smaller_key() {
        let store = InMemoryStore::default();
        let key1 = UUID::gen_v4().with_version(8);
        let key2 = UUID::gen_v4().with_version(8);

        let leaf1 =
            HtreeNode::<u64>::from_kvp(&key1, &1, &store).expect("from_kvp should create leaf1");
        let leaf2 =
            HtreeNode::<u64>::from_kvp(&key2, &2, &store).expect("from_kvp should create leaf2");

        let tree = HtreeNode::from_many_children([leaf1, leaf2], &store)
            .expect("from_many_children should build tree from leaves")
            .into_iter()
            .next()
            .expect("from_many_children should return at least one root node");

        let result = tree.pop_first(&store).expect("pop_first should succeed");

        let min_key = std::cmp::min(key1, key2);
        let max_key = std::cmp::max(key1, key2);

        let popped = result.popped.expect("popped should be Some");
        assert_eq!(popped.key, min_key);
        assert!(popped.is_leaf());
        let remaining = result
            .remaining
            .expect("remaining should contain the larger leaf");

        // The larger key should still be in the tree
        assert!(
            remaining
                .find_one(&max_key, &store)
                .expect("find_one should succeed")
                .is_some()
        );
        // The smaller key should be gone
        assert!(
            remaining
                .find_one(&min_key, &store)
                .expect("find_one should succeed")
                .is_none()
        );
    }

    #[test]
    fn many_leaves_pops_smallest_key() {
        let store = InMemoryStore::default();

        let keys: Vec<_> = (0..10).map(|_| UUID::gen_v4().with_version(8)).collect();

        let leaves: Vec<_> = keys
            .iter()
            .enumerate()
            .map(|(i, k)| {
                HtreeNode::<u64>::from_kvp(k, &(i as u64), &store)
                    .expect("from_kvp should create leaf node")
            })
            .collect();

        let tree = HtreeNode::from_many_children(leaves, &store)
            .expect("from_many_children should build tree from leaves")
            .into_iter()
            .next()
            .expect("from_many_children should return at least one root node");

        let result = tree.pop_first(&store).expect("pop_first should succeed");

        let min_key = *keys.iter().min().expect("keys vector should not be empty");

        let popped = result.popped.expect("popped should be Some");
        assert_eq!(popped.key, min_key);
        assert!(popped.is_leaf());
        let remaining = result
            .remaining
            .expect("remaining should contain all non-popped leaves");

        // The minimum key should no longer be in the tree
        assert!(
            remaining
                .find_one(&min_key, &store)
                .expect("find_one should succeed")
                .is_none()
        );

        // All other keys should still be present
        for key in keys.iter().filter(|&&k| k != min_key) {
            assert!(
                remaining
                    .find_one(key, &store)
                    .expect("find_one should succeed")
                    .is_some()
            );
        }
    }

    #[test]
    fn successive_pop_first_removes_in_ascending_order() {
        let store = InMemoryStore::default();

        let mut keys: Vec<_> = (0..5).map(|_| UUID::gen_v4().with_version(8)).collect();
        keys.sort();

        let leaves: Vec<_> = keys
            .iter()
            .enumerate()
            .map(|(i, k)| {
                HtreeNode::<u64>::from_kvp(k, &(i as u64), &store)
                    .expect("from_kvp should create leaf node")
            })
            .collect();

        let mut tree = HtreeNode::from_many_children(leaves, &store)
            .expect("from_many_children should build tree from leaves")
            .into_iter()
            .next()
            .expect("from_many_children should return at least one root node");

        for expected_key in &keys {
            let result = tree.pop_first(&store).expect("pop_first should succeed");
            let popped = result.popped.expect("popped should be Some");
            assert_eq!(popped.key, *expected_key);
            tree = result.remaining.unwrap_or_default();
        }

        // Tree should now be empty
        assert!(tree.is_empty());
    }

    #[test]
    fn successive_pop_first_removes_in_ascending_order_deep_tree() {
        let store = InMemoryStore::default();

        let pairs: Vec<_> = (0..(MAX_CHILDREN + 5))
            .map(|i| (UUID::gen_v4().with_version(8), i as u64))
            .collect();
        let items: Vec<_> = pairs.iter().map(|(key, value)| (key, value)).collect();

        let mut keys: Vec<_> = pairs.iter().map(|(key, _)| *key).collect();
        keys.sort();

        let mut tree = HtreeNode::from_kvp_iter(items, &store)
            .expect("from_kvp_iter should build a valid deep tree");
        assert!(
            tree.height() > 1,
            "tree should be deep enough for recursion"
        );

        for expected_key in keys {
            let result = tree
                .pop_first(&store)
                .expect("pop_first should succeed on deep tree");
            let popped = result.popped.expect("popped should be Some");
            assert_eq!(popped.key, expected_key);
            tree = result.remaining.unwrap_or_default();
        }

        assert!(tree.is_empty());
    }

    #[test]
    fn pop_first_returns_valid_leaf() {
        let store = InMemoryStore::default();

        let key1 = UUID::gen_v4().with_version(8);
        let key2 = UUID::gen_v4().with_version(8);

        let (smaller_key, larger_key) = if key1 < key2 {
            (key1, key2)
        } else {
            (key2, key1)
        };

        let leaf1 = HtreeNode::<u64>::from_kvp(&smaller_key, &100, &store)
            .expect("from_kvp should create leaf1");
        let leaf2 = HtreeNode::<u64>::from_kvp(&larger_key, &200, &store)
            .expect("from_kvp should create leaf2");

        let tree = HtreeNode::from_many_children([leaf1.clone(), leaf2], &store)
            .expect("from_many_children should build tree from leaves")
            .into_iter()
            .next()
            .expect("from_many_children should return at least one root node");

        let result = tree.pop_first(&store).expect("pop_first should succeed");

        let popped = result.popped.expect("popped should be Some");
        assert_eq!(popped.key, smaller_key);
        assert!(popped.is_leaf());
        // The popped leaf should have the same hkey as the original
        assert_eq!(popped.hkey, leaf1.hkey);
        let remaining = result
            .remaining
            .expect("remaining should contain the larger leaf");

        // Verify remaining entry
        assert!(
            remaining
                .find_one(&larger_key, &store)
                .expect("find_one should succeed")
                .is_some()
        );
    }
}
