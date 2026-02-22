use ps_hkey::Store;

use crate::HtreeNode;

use super::HtreeNodeIterLeavesError;

impl<T> HtreeNode<T> {
    /// Returns the number of leaves in the tree.
    ///
    /// # Performance Warning
    ///
    /// **This is an O(N) operation that traverses the entire tree.** For trees
    /// with millions or billions of entries, this may be prohibitively expensive.
    /// If you need frequent access to the entry count, consider maintaining a
    /// separate counter outside the tree structure.
    ///
    /// # Arguments
    ///
    /// * `store` - The persistence layer providing child node resolution.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`iter_leaves`](Self::iter_leaves):
    /// - [`HtreeNodeIterLeavesError::CorruptedState`] if node state is internally corrupted.
    /// - [`HtreeNodeIterLeavesError::Store`] if store operations fail during child node retrieval.
    /// - [`HtreeNodeIterLeavesError::UnpackChildren`] if unpacking child nodes fails.
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
    /// // Empty tree has count 0
    /// let empty: HtreeNode<u64> = HtreeNode::default();
    /// assert_eq!(empty.count_leaves(&store).unwrap(), 0);
    ///
    /// // Single leaf has count 1
    /// let key = UUID::gen_v4();
    /// let leaf = HtreeNode::<u64>::from_kvp(&key, &42, &store).unwrap();
    /// assert_eq!(leaf.count_leaves(&store).unwrap(), 1);
    /// ```
    pub fn count_leaves<S: Store>(&self, store: &S) -> Result<usize, HtreeNodeIterLeavesError<S>> {
        let mut count = 0;
        for result in self.iter_leaves(store) {
            result?;
            count += 1;
        }
        Ok(count)
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use ps_hkey::InMemoryStore;
    use ps_uuid::UUID;

    use crate::HtreeNode;

    #[test]
    fn empty_tree_has_count_zero() {
        let store = InMemoryStore::default();
        let tree: HtreeNode<u64> = HtreeNode::default();

        assert_eq!(
            tree.count_leaves(&store)
                .expect("count_leaves should not fail on empty tree"),
            0
        );
    }

    #[test]
    fn single_leaf_has_count_one() {
        let store = InMemoryStore::default();
        let key = UUID::gen_v4();

        let tree = HtreeNode::<u64>::from_kvp(&key, &42, &store)
            .expect("from_kvp should create a valid leaf node");

        assert_eq!(
            tree.count_leaves(&store)
                .expect("count_leaves should succeed"),
            1
        );
    }

    #[test]
    fn multi_leaf_tree_counts_all_leaves() {
        let store = InMemoryStore::default();

        let keys: Vec<UUID> = (0..10).map(|_| UUID::gen_v4()).collect();
        let leaves: Vec<_> = keys
            .iter()
            .enumerate()
            .map(|(i, k)| {
                HtreeNode::<u64>::from_kvp(k, &(i as u64), &store)
                    .expect("from_kvp should create leaf node")
            })
            .collect();

        let tree = HtreeNode::from_many_children(leaves, &store)
            .expect("from_many_children should build tree")
            .into_iter()
            .next()
            .expect("should return at least one root node");

        assert_eq!(
            tree.count_leaves(&store)
                .expect("count_leaves should succeed"),
            10
        );
    }

    #[test]
    fn count_after_deletion() {
        let store = InMemoryStore::default();

        let keys: Vec<UUID> = (0..5).map(|_| UUID::gen_v4()).collect();
        let leaves: Vec<_> = keys
            .iter()
            .enumerate()
            .map(|(i, k)| {
                HtreeNode::<u64>::from_kvp(k, &(i as u64), &store)
                    .expect("from_kvp should create leaf node")
            })
            .collect();

        let tree = HtreeNode::from_many_children(leaves, &store)
            .expect("from_many_children should build tree")
            .into_iter()
            .next()
            .expect("should return at least one root node");

        let tree = tree
            .delete_one(&keys[0], &store)
            .expect("delete_one should succeed");

        assert_eq!(
            tree.count_leaves(&store)
                .expect("count_leaves should succeed"),
            4
        );
    }

    #[test]
    fn count_of_large_tree() {
        let store = InMemoryStore::default();

        let keys: Vec<UUID> = (0..100).map(|_| UUID::gen_v4()).collect();
        let leaves: Vec<_> = keys
            .iter()
            .enumerate()
            .map(|(i, k)| {
                HtreeNode::<u64>::from_kvp(k, &(i as u64), &store)
                    .expect("from_kvp should create leaf node")
            })
            .collect();

        // Keep combining until we get a single root
        let mut nodes = HtreeNode::from_many_children(leaves, &store)
            .expect("from_many_children should build tree");
        while nodes.len() > 1 {
            nodes = HtreeNode::from_many_children(nodes, &store)
                .expect("from_many_children should succeed");
        }
        let tree = nodes.into_iter().next().expect("should have a root node");

        assert_eq!(
            tree.count_leaves(&store)
                .expect("count_leaves should succeed"),
            100
        );
    }
}
