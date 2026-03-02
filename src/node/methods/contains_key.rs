use ps_hkey::Store;

use crate::{HtreeKey, HtreeNode};

use super::HtreeNodeFindOneError;

impl<T> HtreeNode<T> {
    /// Returns `true` if this tree contains a leaf with the given key.
    ///
    /// This is a convenience wrapper around [`find_one`](Self::find_one) that
    /// discards the returned node and returns only whether a match was found.
    ///
    /// # Arguments
    ///
    /// * `key` - The key to search for. Must implement [`HtreeKey`] for UUID conversion via the store.
    /// * `store` - The persistence layer providing key conversion and child node resolution.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`find_one`](Self::find_one):
    /// - [`HtreeNodeFindOneError::Key`] if key conversion to a UUID fails.
    /// - [`HtreeNodeFindOneError::Store`] if store operations fail during key conversion or child node retrieval.
    /// - [`HtreeNodeFindOneError::UnpackChildren`] if unpacking child nodes fails during tree traversal.
    ///
    /// # Examples
    ///
    /// ```
    /// use ps_htree::HtreeNode;
    /// use ps_hkey::InMemoryStore;
    /// use ps_uuid::UUID;
    ///
    /// let store = InMemoryStore::default();
    /// let key = UUID::gen_v4();
    ///
    /// let tree = HtreeNode::<u64>::from_kvp(&key, &42, &store).unwrap();
    /// assert!(tree.contains_key(&key, &store).unwrap());
    ///
    /// let other_key = UUID::gen_v4();
    /// assert!(!tree.contains_key(&other_key, &store).unwrap());
    /// ```
    pub fn contains_key<K: HtreeKey + ?Sized, S: Store>(
        &self,
        key: &K,
        store: &S,
    ) -> Result<bool, HtreeNodeFindOneError<S>> {
        Ok(self.find_one(key, store)?.is_some())
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use ps_hkey::InMemoryStore;
    use ps_uuid::UUID;

    use crate::HtreeNode;

    #[test]
    fn empty_tree_contains_nothing() {
        let store = InMemoryStore::default();
        let tree: HtreeNode<()> = HtreeNode::default();
        let key = UUID::gen_v4().with_version(8);

        assert!(
            !tree
                .contains_key(&key, &store)
                .expect("contains_key should not fail on empty tree")
        );
    }

    #[test]
    fn single_leaf_contains_its_key() {
        let store = InMemoryStore::default();
        let key = UUID::gen_v4().with_version(8);

        let tree = HtreeNode::<u64>::from_kvp(&key, &42, &store)
            .expect("from_kvp should create a valid leaf node");
        assert!(
            tree.contains_key(&key, &store)
                .expect("contains_key should find the key that was just inserted")
        );
    }

    #[test]
    fn single_leaf_does_not_contain_other_key() {
        let store = InMemoryStore::default();
        let key = UUID::gen_v4().with_version(8);
        let other_key = UUID::gen_v4().with_version(8);

        let tree = HtreeNode::<u64>::from_kvp(&key, &42, &store)
            .expect("from_kvp should create a valid leaf node");
        assert!(
            !tree
                .contains_key(&other_key, &store)
                .expect("contains_key should not fail when checking for missing key")
        );
    }

    #[test]
    fn multi_leaf_tree_contains_all_its_keys() {
        let store = InMemoryStore::default();
        let key1 = UUID::gen_v4().with_version(8);
        let key2 = UUID::gen_v4().with_version(8);
        let key3 = UUID::gen_v4().with_version(8);

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

        assert!(
            tree.contains_key(&key1, &store)
                .expect("tree should contain key1")
        );
        assert!(
            tree.contains_key(&key2, &store)
                .expect("tree should contain key2")
        );
        assert!(
            tree.contains_key(&key3, &store)
                .expect("tree should contain key3")
        );
    }

    #[test]
    fn multi_leaf_tree_does_not_contain_missing_key() {
        let store = InMemoryStore::default();
        let key1 = UUID::gen_v4().with_version(8);
        let key2 = UUID::gen_v4().with_version(8);
        let missing_key = UUID::gen_v4().with_version(8);

        let leaf1 =
            HtreeNode::<u64>::from_kvp(&key1, &1, &store).expect("from_kvp should create leaf1");
        let leaf2 =
            HtreeNode::<u64>::from_kvp(&key2, &2, &store).expect("from_kvp should create leaf2");

        let tree = HtreeNode::from_many_children([leaf1, leaf2], &store)
            .expect("from_many_children should build tree from leaves")
            .into_iter()
            .next()
            .expect("from_many_children should return at least one root node");

        assert!(
            !tree
                .contains_key(&missing_key, &store)
                .expect("contains_key should not fail when checking for missing key")
        );
    }

    #[test]
    fn contains_key_after_deletion_returns_false() {
        let store = InMemoryStore::default();
        let key = UUID::gen_v4().with_version(8);

        let tree = HtreeNode::<u64>::from_kvp(&key, &42, &store)
            .expect("from_kvp should create a valid leaf node");
        assert!(
            tree.contains_key(&key, &store)
                .expect("tree should contain the key before deletion")
        );

        let tree = tree
            .delete_one(&key, &store)
            .expect("delete_one should succeed");
        assert!(
            !tree
                .contains_key(&key, &store)
                .expect("tree should not contain the key after deletion")
        );
    }

    #[test]
    fn contains_key_after_partial_deletion() {
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

        let tree = tree
            .delete_one(&key1, &store)
            .expect("delete_one should succeed");

        assert!(
            !tree
                .contains_key(&key1, &store)
                .expect("tree should not contain key1 after deletion")
        );
        assert!(
            tree.contains_key(&key2, &store)
                .expect("tree should still contain key2 after deleting key1")
        );
    }
}
