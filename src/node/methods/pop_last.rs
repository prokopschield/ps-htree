use ps_hkey::Store;

use crate::{HtreeNode, PopResult};

impl<T> HtreeNode<T> {
    /// Removes and returns the leaf with the largest key from the tree.
    ///
    /// This is analogous to [`BTreeMap::pop_last`](std::collections::BTreeMap::pop_last).
    /// It finds the maximum key in the tree, removes its entry, and returns both
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
    /// - `remaining`: The tree after removal (empty if nothing remains)
    ///
    /// # Errors
    ///
    /// - [`HtreeNodePopLastError::FetchChildren`] if children cannot be fetched during traversal.
    /// - [`HtreeNodePopLastError::FromChildren`] if tree reconstruction fails.
    /// - [`HtreeNodePopLastError::Store`] if store operations fail.
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
    /// assert!(empty.pop_last(&store).unwrap().popped.is_none());
    ///
    /// // Tree with entries returns the maximum
    /// let key1 = UUID::gen_v4().with_version(8);
    /// let key2 = UUID::gen_v4().with_version(8);
    /// let max_key = std::cmp::max(key1, key2);
    ///
    /// let leaf1 = HtreeNode::<u64>::from_kvp(&key1, &1, &store).unwrap();
    /// let leaf2 = HtreeNode::<u64>::from_kvp(&key2, &2, &store).unwrap();
    /// let tree = HtreeNode::from_many_children([leaf1, leaf2], &store)
    ///     .unwrap()
    ///     .into_iter()
    ///     .next()
    ///     .unwrap();
    ///
    /// let result = tree.pop_last(&store).unwrap();
    /// let popped = result.popped.unwrap();
    /// assert_eq!(popped.key, max_key);
    /// assert!(result.remaining.find_one(&max_key, &store).unwrap().is_none());
    /// ```
    pub fn pop_last<S: Store>(self, store: &S) -> Result<PopResult<T>, HtreeNodePopLastError<S>> {
        if self.is_empty() {
            return Ok(PopResult {
                popped: None,
                remaining: Self::default(),
            });
        }

        if self.is_leaf() {
            return Ok(PopResult {
                popped: Some(self),
                remaining: Self::default(),
            });
        }

        self.pop_last_internal(store)
    }

    fn pop_last_internal<S: Store>(
        self,
        store: &S,
    ) -> Result<PopResult<T>, HtreeNodePopLastError<S>> {
        let children = self.fetch_children(store)?;
        let mut children_vec: Vec<Self> = children.into_iter().collect();

        while let Some(last_child) = children_vec.pop() {
            let PopResult { popped, remaining } = last_child.pop_last(store)?;

            let Some(popped) = popped else {
                continue;
            };

            // Rebuild tree: include remaining only if non-empty
            if !remaining.is_empty() {
                children_vec.push(remaining);
            }

            let remaining = Self::from_children(children_vec, store)?;

            return Ok(PopResult {
                popped: Some(popped),
                remaining,
            });
        }

        Ok(PopResult {
            popped: None,
            remaining: Self::default(),
        })
    }
}

#[derive(thiserror::Error, Debug)]
pub enum HtreeNodePopLastError<S: Store> {
    #[error(transparent)]
    FetchChildren(crate::HtreeNodeFetchChildrenError<S>),

    #[error(transparent)]
    FromChildren(crate::HtreeNodeFromChildrenError<S>),

    #[error("Store error: {0}")]
    Store(S::Error),
}

impl<S: Store> From<crate::HtreeNodeFetchChildrenError<S>> for HtreeNodePopLastError<S> {
    fn from(value: crate::HtreeNodeFetchChildrenError<S>) -> Self {
        match value {
            crate::HtreeNodeFetchChildrenError::Store(err) => Self::Store(err),
            err => Self::FetchChildren(err),
        }
    }
}

impl<S: Store> From<crate::HtreeNodeFromChildrenError<S>> for HtreeNodePopLastError<S> {
    fn from(value: crate::HtreeNodeFromChildrenError<S>) -> Self {
        match value {
            crate::HtreeNodeFromChildrenError::Store(err) => Self::Store(err),
            err => Self::FromChildren(err),
        }
    }
}

#[allow(clippy::expect_used)]
#[cfg(test)]
mod tests {
    use ps_hkey::InMemoryStore;
    use ps_uuid::UUID;

    use crate::{HtreeNode, MAX_CHILDREN};

    #[test]
    fn empty_tree_returns_none() {
        let store = InMemoryStore::default();
        let tree: HtreeNode<u64> = HtreeNode::default();

        let result = tree
            .pop_last(&store)
            .expect("pop_last should not fail on empty tree");

        assert!(result.popped.is_none());
        assert!(result.remaining.is_empty());
    }

    #[test]
    fn single_leaf_pops_and_returns_empty() {
        let store = InMemoryStore::default();
        let key = UUID::gen_v4().with_version(8);
        let value = 42_u64;

        let tree = HtreeNode::from_kvp(&key, &value, &store)
            .expect("from_kvp should create a valid leaf node");

        let result = tree
            .pop_last(&store)
            .expect("pop_last should succeed on single leaf tree");

        let popped = result.popped.expect("popped should be Some");
        assert_eq!(popped.key, key);
        assert!(popped.is_leaf());
        assert!(result.remaining.is_empty());
    }

    #[test]
    fn two_leaves_pops_larger_key() {
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

        let result = tree.pop_last(&store).expect("pop_last should succeed");

        let min_key = std::cmp::min(key1, key2);
        let max_key = std::cmp::max(key1, key2);

        let popped = result.popped.expect("popped should be Some");
        assert_eq!(popped.key, max_key);
        assert!(popped.is_leaf());

        // The smaller key should still be in the tree
        assert!(
            result
                .remaining
                .find_one(&min_key, &store)
                .expect("find_one should succeed")
                .is_some()
        );
        // The larger key should be gone
        assert!(
            result
                .remaining
                .find_one(&max_key, &store)
                .expect("find_one should succeed")
                .is_none()
        );
    }

    #[test]
    fn many_leaves_pops_largest_key() {
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

        let result = tree.pop_last(&store).expect("pop_last should succeed");

        let max_key = *keys.iter().max().expect("keys vector should not be empty");

        let popped = result.popped.expect("popped should be Some");
        assert_eq!(popped.key, max_key);
        assert!(popped.is_leaf());

        // The maximum key should no longer be in the tree
        assert!(
            result
                .remaining
                .find_one(&max_key, &store)
                .expect("find_one should succeed")
                .is_none()
        );

        // All other keys should still be present
        for key in keys.iter().filter(|&&k| k != max_key) {
            assert!(
                result
                    .remaining
                    .find_one(key, &store)
                    .expect("find_one should succeed")
                    .is_some()
            );
        }
    }

    #[test]
    fn successive_pop_last_removes_in_descending_order() {
        let store = InMemoryStore::default();

        let mut keys: Vec<_> = (0..5).map(|_| UUID::gen_v4().with_version(8)).collect();
        keys.sort();
        keys.reverse(); // Descending order

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
            let result = tree.pop_last(&store).expect("pop_last should succeed");
            let popped = result.popped.expect("popped should be Some");
            assert_eq!(popped.key, *expected_key);
            tree = result.remaining;
        }

        // Tree should now be empty
        assert!(tree.is_empty());
    }

    #[test]
    fn successive_pop_last_removes_in_descending_order_deep_tree() {
        let store = InMemoryStore::default();

        let pairs: Vec<_> = (0..(MAX_CHILDREN + 5))
            .map(|i| (UUID::gen_v4().with_version(8), i as u64))
            .collect();
        let items: Vec<_> = pairs.iter().map(|(key, value)| (key, value)).collect();

        let mut keys: Vec<_> = pairs.iter().map(|(key, _)| *key).collect();
        keys.sort();
        keys.reverse();

        let mut tree = HtreeNode::from_kvp_iter(items, &store)
            .expect("from_kvp_iter should build a valid deep tree");
        assert!(
            tree.height() > 1,
            "tree should be deep enough for recursion"
        );

        for expected_key in keys {
            let result = tree
                .pop_last(&store)
                .expect("pop_last should succeed on deep tree");
            let popped = result.popped.expect("popped should be Some");
            assert_eq!(popped.key, expected_key);
            tree = result.remaining;
        }

        assert!(tree.is_empty());
    }

    #[test]
    fn pop_last_returns_valid_leaf() {
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

        let tree = HtreeNode::from_many_children([leaf1, leaf2.clone()], &store)
            .expect("from_many_children should build tree from leaves")
            .into_iter()
            .next()
            .expect("from_many_children should return at least one root node");

        let result = tree.pop_last(&store).expect("pop_last should succeed");

        let popped = result.popped.expect("popped should be Some");
        assert_eq!(popped.key, larger_key);
        assert!(popped.is_leaf());
        // The popped leaf should have the same hkey as the original
        assert_eq!(popped.hkey, leaf2.hkey);

        // Verify remaining entry
        assert!(
            result
                .remaining
                .find_one(&smaller_key, &store)
                .expect("find_one should succeed")
                .is_some()
        );
    }

    #[test]
    fn pop_first_and_pop_last_on_same_tree() {
        let store = InMemoryStore::default();

        let mut keys: Vec<_> = (0..4).map(|_| UUID::gen_v4().with_version(8)).collect();
        keys.sort();

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

        // Pop first (smallest)
        let result = tree.pop_first(&store).expect("pop_first should succeed");
        let popped = result.popped.expect("popped should be Some");
        assert_eq!(popped.key, keys[0]);

        // Pop last (largest)
        let result = result
            .remaining
            .pop_last(&store)
            .expect("pop_last should succeed");
        let popped = result.popped.expect("popped should be Some");
        assert_eq!(popped.key, keys[3]);

        // Middle two keys should remain
        assert!(
            result
                .remaining
                .find_one(&keys[1], &store)
                .expect("find_one should succeed")
                .is_some()
        );
        assert!(
            result
                .remaining
                .find_one(&keys[2], &store)
                .expect("find_one should succeed")
                .is_some()
        );
    }
}
