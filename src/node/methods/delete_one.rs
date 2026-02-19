use ps_hkey::Store;

use crate::{HtreeKey, HtreeNode};

impl<T> HtreeNode<T> {
    /// Deletes a single key from the tree.
    ///
    /// Equivalent to [`delete_many`](Self::delete_many) with one key.
    /// This operation is **idempotent**: deleting a non-existent key returns the tree unchanged.
    ///
    /// # Arguments
    /// * `key` - Key reference to delete
    /// * `store` - Persistence layer
    ///
    /// # Errors
    /// - [`Store`](HtreeNodeDeleteOneError::Store) if persistence fails.
    /// - [`Key`](HtreeNodeDeleteOneError::Key) if key conversion fails.
    /// - [`DeleteMany`](HtreeNodeDeleteOneError::DeleteMany) if deletion fails.
    pub fn delete_one<S: Store>(
        &self,
        key: &impl HtreeKey,
        store: &S,
    ) -> Result<Self, HtreeNodeDeleteOneError<S>> {
        Ok(self.delete_many([key], store)?)
    }
}

#[derive(thiserror::Error, Debug)]
pub enum HtreeNodeDeleteOneError<S: Store> {
    #[error(transparent)]
    DeleteMany(crate::HtreeNodeDeleteManyError<S>),
    #[error("Key error: {0}")]
    Key(crate::HtreeKeyError<S>),
    #[error("Store error: {0}")]
    Store(S::Error),
}

impl<S: Store> From<crate::HtreeNodeDeleteManyError<S>> for HtreeNodeDeleteOneError<S> {
    fn from(value: crate::HtreeNodeDeleteManyError<S>) -> Self {
        match value {
            crate::HtreeNodeDeleteManyError::Key(err) => Self::Key(err),
            crate::HtreeNodeDeleteManyError::Store(err) => Self::Store(err),
            err => Self::DeleteMany(err),
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
    fn delete_existing_key() {
        let store = InMemoryStore::default();
        let key = UUID::gen_v4().with_version(8);

        let tree = HtreeNode::<u64>::from_kvp(&key, &42, &store)
            .expect("from_kvp should create a valid leaf node");
        assert!(
            tree.find_one(&key, &store)
                .expect("find_one should succeed")
                .is_some()
        );

        let tree = tree
            .delete_one(&key, &store)
            .expect("delete_one should succeed");
        assert!(
            tree.find_one(&key, &store)
                .expect("find_one should succeed after deletion")
                .is_none()
        );
    }

    #[test]
    fn delete_nonexistent_key_is_idempotent() {
        let store = InMemoryStore::default();
        let key1 = UUID::gen_v4().with_version(8);
        let key2 = UUID::gen_v4().with_version(8);

        let tree = HtreeNode::<u64>::from_kvp(&key1, &42, &store)
            .expect("from_kvp should create a valid leaf node");
        let tree_before = tree.clone();

        let tree_after = tree
            .delete_one(&key2, &store)
            .expect("delete_one should succeed even for nonexistent key");

        assert_eq!(tree_before.hkey, tree_after.hkey);
        assert!(
            tree_after
                .find_one(&key1, &store)
                .expect("find_one should succeed")
                .is_some()
        );
    }

    #[test]
    fn delete_from_multi_leaf_tree() {
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
            tree.find_one(&key1, &store)
                .expect("find_one should find key1")
                .is_some()
        );
        assert!(
            tree.find_one(&key2, &store)
                .expect("find_one should find key2")
                .is_some()
        );
        assert!(
            tree.find_one(&key3, &store)
                .expect("find_one should find key3")
                .is_some()
        );

        let tree = tree
            .delete_one(&key2, &store)
            .expect("delete_one should succeed");

        assert!(
            tree.find_one(&key1, &store)
                .expect("find_one should still find key1")
                .is_some()
        );
        assert!(
            tree.find_one(&key2, &store)
                .expect("find_one should succeed but return None for deleted key2")
                .is_none()
        );
        assert!(
            tree.find_one(&key3, &store)
                .expect("find_one should still find key3")
                .is_some()
        );
    }

    #[test]
    fn delete_all_leaves_returns_default() {
        let store = InMemoryStore::default();
        let key = UUID::gen_v4().with_version(8);

        let tree = HtreeNode::<u64>::from_kvp(&key, &42, &store)
            .expect("from_kvp should create a valid leaf node");
        let tree = tree
            .delete_one(&key, &store)
            .expect("delete_one should succeed");

        assert_eq!(tree, HtreeNode::default());
    }

    #[test]
    fn double_delete_is_idempotent() {
        let store = InMemoryStore::default();
        let key = UUID::gen_v4().with_version(8);

        let tree = HtreeNode::<u64>::from_kvp(&key, &42, &store)
            .expect("from_kvp should create a valid leaf node");
        let tree = tree
            .delete_one(&key, &store)
            .expect("first delete_one should succeed");
        let tree = tree
            .delete_one(&key, &store)
            .expect("second delete_one should succeed (idempotent)");

        assert_eq!(tree, HtreeNode::default());
    }
}
