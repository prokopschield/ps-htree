use ps_hkey::Store;

use crate::HtreeNode;

impl<T> HtreeNode<T> {
    /// Returns the leaf with the smallest key in the tree.
    ///
    /// Descends the leftmost path from root to leaf with O(height) traversal,
    /// fetching O(1) nodes per level.
    ///
    /// Returns `None` only if the tree is empty (i.e., `is_empty()` is true).
    /// For a non-empty tree, always returns `Some(leaf)`.
    ///
    /// # Arguments
    /// * `store` - persistence backend
    ///
    /// # Errors
    /// Returns [`HtreeNodeFirstError`] if child iteration fails during traversal.
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
    /// let empty: HtreeNode<()> = HtreeNode::default();
    /// assert!(empty.first(&store).unwrap().is_none());
    ///
    /// // Single leaf returns itself
    /// let key = UUID::gen_v4().with_version(8);
    /// let leaf = HtreeNode::<u64>::from_kvp(&key, &42, &store).unwrap();
    /// let first = leaf.first(&store).unwrap().unwrap();
    /// assert_eq!(first.key, key);
    /// ```
    pub fn first<S: Store>(&self, store: &S) -> Result<Option<Self>, HtreeNodeFirstError<S>> {
        self.iter_leaves(store)
            .next()
            .transpose()
            .map_err(Into::into)
    }
}

#[derive(thiserror::Error, Debug)]
pub enum HtreeNodeFirstError<S: Store> {
    #[error("HtreeNode's state is internally corrupted.")]
    CorruptedState,
    #[error("Store error: {0}")]
    Store(S::Error),
    #[error(transparent)]
    UnpackChildren(#[from] crate::HtreeNodeUnpackChildrenError),
}

impl<S: Store> From<crate::HtreeNodeIterLeavesError<S>> for HtreeNodeFirstError<S> {
    fn from(value: crate::HtreeNodeIterLeavesError<S>) -> Self {
        match value {
            crate::HtreeNodeIterLeavesError::CorruptedState => Self::CorruptedState,
            crate::HtreeNodeIterLeavesError::Store(err) => Self::Store(err),
            crate::HtreeNodeIterLeavesError::UnpackChildren(err) => Self::UnpackChildren(err),
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use ps_hkey::InMemoryStore;
    use ps_uuid::UUID;

    use crate::HtreeNode;

    #[test]
    fn empty_tree_returns_none() {
        let store = InMemoryStore::default();
        let tree: HtreeNode<()> = HtreeNode::default();
        assert!(
            tree.first(&store)
                .expect("first should not fail on empty tree")
                .is_none()
        );
    }

    #[test]
    fn single_leaf_returns_itself() {
        let store = InMemoryStore::default();
        let key = UUID::gen_v4().with_version(8);

        let tree = HtreeNode::<u64>::from_kvp(&key, &42, &store)
            .expect("from_kvp should create a valid leaf node");
        let first = tree
            .first(&store)
            .expect("first should succeed on single leaf tree")
            .expect("first should return Some for non-empty tree");

        assert_eq!(first.key, key);
        assert!(first.is_leaf());
    }

    #[test]
    fn two_leaves_returns_smaller_key() {
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

        let first = tree
            .first(&store)
            .expect("first should succeed")
            .expect("first should return Some for non-empty tree");
        let expected_key = std::cmp::min(key1, key2);

        assert_eq!(first.key, expected_key);
        assert!(first.is_leaf());
    }

    #[test]
    fn many_leaves_returns_smallest_key() {
        let store = InMemoryStore::default();

        // Generate multiple keys
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

        let first = tree
            .first(&store)
            .expect("first should succeed")
            .expect("first should return Some for non-empty tree");
        let expected_key = *keys.iter().min().expect("keys vector should not be empty");

        assert_eq!(first.key, expected_key);
        assert!(first.is_leaf());
    }

    #[test]
    fn first_on_leaf_returns_self() {
        let store = InMemoryStore::default();
        let key = UUID::gen_v4().with_version(8);

        let leaf = HtreeNode::<u64>::from_kvp(&key, &42, &store)
            .expect("from_kvp should create a valid leaf node");
        assert!(leaf.is_leaf());

        let first = leaf
            .first(&store)
            .expect("first should succeed")
            .expect("first should return Some for non-empty tree");
        assert_eq!(first.key, leaf.key);
    }
}
