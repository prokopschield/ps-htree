use ps_hkey::Store;
use ps_uuid::UUID;

use crate::HtreeNode;

use super::HtreeNodeIterLeavesError;

impl<T> HtreeNode<T> {
    /// Returns an iterator over all keys (UUIDs) in this tree.
    ///
    /// This is a thin adapter over [`iter_leaves`](Self::iter_leaves) that maps
    /// each leaf node to its key. Keys are yielded in sorted order (smallest to largest).
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
    /// let key1 = UUID::gen_v4();
    /// let key2 = UUID::gen_v4();
    ///
    /// let leaf1 = HtreeNode::<u64>::from_kvp(&key1, &1, &store).unwrap();
    /// let leaf2 = HtreeNode::<u64>::from_kvp(&key2, &2, &store).unwrap();
    ///
    /// let tree = HtreeNode::from_many_children([leaf1, leaf2], &store)
    ///     .unwrap()
    ///     .into_iter()
    ///     .next()
    ///     .unwrap();
    ///
    /// let keys: Vec<UUID> = tree.iter_keys(&store).map(|r| r.unwrap()).collect();
    /// assert_eq!(keys.len(), 2);
    /// ```
    pub fn iter_keys<'a, S: Store>(
        &'a self,
        store: &'a S,
    ) -> impl Iterator<Item = Result<UUID, HtreeNodeIterLeavesError<S>>> + 'a {
        self.iter_leaves(store)
            .map(|item| item.map(|item| item.key))
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use ps_hkey::InMemoryStore;
    use ps_uuid::UUID;

    use crate::HtreeNode;

    #[test]
    fn empty_tree_yields_no_keys() {
        let store = InMemoryStore::default();
        let tree: HtreeNode<()> = HtreeNode::default();

        assert!(tree.iter_keys(&store).next().is_none());
    }

    #[test]
    fn single_leaf_yields_one_key() {
        let store = InMemoryStore::default();
        let key = UUID::gen_v4();

        let tree = HtreeNode::<u64>::from_kvp(&key, &42, &store)
            .expect("from_kvp should create a valid leaf node");

        let keys: Vec<_> = tree
            .iter_keys(&store)
            .map(|r| r.expect("iter_keys should not fail"))
            .collect();
        assert_eq!(keys, vec![key]);
    }

    #[test]
    fn multi_leaf_tree_yields_all_keys() {
        let store = InMemoryStore::default();
        let key1 = UUID::gen_v4();
        let key2 = UUID::gen_v4();
        let key3 = UUID::gen_v4();

        let leaf1 =
            HtreeNode::<u64>::from_kvp(&key1, &1, &store).expect("from_kvp should create leaf1");
        let leaf2 =
            HtreeNode::<u64>::from_kvp(&key2, &2, &store).expect("from_kvp should create leaf2");
        let leaf3 =
            HtreeNode::<u64>::from_kvp(&key3, &3, &store).expect("from_kvp should create leaf3");

        let tree = HtreeNode::from_many_children([leaf1, leaf2, leaf3], &store)
            .expect("from_many_children should build tree from leaves")
            .into_iter()
            .next()
            .expect("from_many_children should return at least one root node");

        let collected_keys: Vec<_> = tree
            .iter_keys(&store)
            .map(|r| r.expect("iter_keys should not fail"))
            .collect();
        assert_eq!(collected_keys.len(), 3);
        assert!(collected_keys.contains(&key1));
        assert!(collected_keys.contains(&key2));
        assert!(collected_keys.contains(&key3));
    }

    #[test]
    fn keys_are_yielded_in_sorted_order() {
        let store = InMemoryStore::default();

        let mut original_keys: Vec<UUID> = (0..10).map(|_| UUID::gen_v4()).collect();

        let leaves: Vec<_> = original_keys
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

        let collected_keys: Vec<_> = tree
            .iter_keys(&store)
            .map(|r| r.expect("iter_keys should not fail"))
            .collect();

        let mut sorted_keys = collected_keys.clone();
        sorted_keys.sort();
        assert_eq!(collected_keys, sorted_keys);

        original_keys.sort();
        assert_eq!(collected_keys, original_keys);
    }

    #[test]
    fn keys_after_deletion() {
        let store = InMemoryStore::default();
        let key1 = UUID::gen_v4();
        let key2 = UUID::gen_v4();

        let leaf1 =
            HtreeNode::<u64>::from_kvp(&key1, &1, &store).expect("from_kvp should create leaf1");
        let leaf2 =
            HtreeNode::<u64>::from_kvp(&key2, &2, &store).expect("from_kvp should create leaf2");

        let tree = HtreeNode::from_many_children([leaf1, leaf2], &store)
            .expect("from_many_children should build tree from leaves")
            .into_iter()
            .next()
            .expect("from_many_children should return at least one root node");

        let tree = tree
            .delete_one(&key1, &store)
            .expect("delete_one should succeed");

        let collected_keys: Vec<_> = tree
            .iter_keys(&store)
            .map(|r| r.expect("iter_keys should not fail"))
            .collect();
        assert_eq!(collected_keys, vec![key2]);
    }
}
