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
#[allow(clippy::unwrap_used)]
mod tests {
    use ps_hkey::InMemoryStore;
    use ps_uuid::UUID;

    use crate::HtreeNode;

    #[test]
    fn delete_existing_key() {
        let store = InMemoryStore::default();
        let key = UUID::gen_v4().with_version(8);

        let tree = HtreeNode::<u64>::from_kvp(&key, &42, &store).unwrap();
        assert!(tree.find_one(&key, &store).unwrap().is_some());

        let tree = tree.delete_one(&key, &store).unwrap();
        assert!(tree.find_one(&key, &store).unwrap().is_none());
    }

    #[test]
    fn delete_nonexistent_key_is_idempotent() {
        let store = InMemoryStore::default();
        let key1 = UUID::gen_v4().with_version(8);
        let key2 = UUID::gen_v4().with_version(8);

        let tree = HtreeNode::<u64>::from_kvp(&key1, &42, &store).unwrap();
        let tree_before = tree.clone();

        let tree_after = tree.delete_one(&key2, &store).unwrap();

        assert_eq!(tree_before.hkey, tree_after.hkey);
        assert!(tree_after.find_one(&key1, &store).unwrap().is_some());
    }

    #[test]
    fn delete_from_multi_leaf_tree() {
        let store = InMemoryStore::default();
        let key1 = UUID::gen_v4().with_version(8);
        let key2 = UUID::gen_v4().with_version(8);
        let key3 = UUID::gen_v4().with_version(8);

        let leaf1 = HtreeNode::<u64>::from_kvp(&key1, &1, &store).unwrap();
        let leaf2 = HtreeNode::<u64>::from_kvp(&key2, &2, &store).unwrap();
        let leaf3 = HtreeNode::<u64>::from_kvp(&key3, &3, &store).unwrap();

        let tree = HtreeNode::from_many_children([leaf1, leaf2, leaf3], &store)
            .unwrap()
            .into_iter()
            .next()
            .unwrap();

        assert!(tree.find_one(&key1, &store).unwrap().is_some());
        assert!(tree.find_one(&key2, &store).unwrap().is_some());
        assert!(tree.find_one(&key3, &store).unwrap().is_some());

        let tree = tree.delete_one(&key2, &store).unwrap();

        assert!(tree.find_one(&key1, &store).unwrap().is_some());
        assert!(tree.find_one(&key2, &store).unwrap().is_none());
        assert!(tree.find_one(&key3, &store).unwrap().is_some());
    }

    #[test]
    fn delete_all_leaves_returns_default() {
        let store = InMemoryStore::default();
        let key = UUID::gen_v4().with_version(8);

        let tree = HtreeNode::<u64>::from_kvp(&key, &42, &store).unwrap();
        let tree = tree.delete_one(&key, &store).unwrap();

        assert_eq!(tree, HtreeNode::default());
    }

    #[test]
    fn double_delete_is_idempotent() {
        let store = InMemoryStore::default();
        let key = UUID::gen_v4().with_version(8);

        let tree = HtreeNode::<u64>::from_kvp(&key, &42, &store).unwrap();
        let tree = tree.delete_one(&key, &store).unwrap();
        let tree = tree.delete_one(&key, &store).unwrap();

        assert_eq!(tree, HtreeNode::default());
    }
}
