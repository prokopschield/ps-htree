use ps_hkey::Store;

use crate::{HtreeKey, HtreeNode};

impl<T> HtreeNode<T> {
    /// Finds and returns a leaf node matching the given key by descending the tree.
    ///
    /// Starting from the current node, this method recursively selects child nodes whose key range
    /// contains the target key until reaching a leaf. Returns `Ok(Some(leaf))` if a leaf with
    /// matching key is found, or `Ok(None)` if no matching leaf exists in the subtree rooted at
    /// this node. If tree traversal cannot continue due to corrupted node state, gracefully returns
    /// `Ok(None)`.
    ///
    /// # Arguments
    ///
    /// * `key` - The key to search for. Must implement [`HtreeKey`] for UUID conversion via the store.
    /// * `store` - The persistence layer providing key conversion and child node resolution.
    ///
    /// # Errors
    ///
    /// - [`HtreeNodeFindOneError::Key`] is returned if key conversion to a UUID fails.
    /// - [`HtreeNodeFindOneError::Store`] is returned if store operations fail during key conversion or child node retrieval.
    /// - [`HtreeNodeFindOneError::UnpackChildren`] is returned if unpacking child nodes fails during tree traversal, indicating corrupted or invalid persisted state.
    pub fn find_one<K: HtreeKey + ?Sized, S: Store>(
        &self,
        key: &K,
        store: &S,
    ) -> Result<Option<Self>, HtreeNodeFindOneError<S>> {
        let key = key.try_to_uuid(store)?;

        let Some(mut node) = self.select_child(&key, store)? else {
            return Ok(None);
        };

        loop {
            if node.is_leaf() && node.key == key {
                return Ok(Some(node));
            }

            if let Some(child) = node.select_child(&key, store)? {
                node = child;
            } else {
                return Ok(None);
            }
        }
    }
}

#[derive(thiserror::Error, Debug)]
pub enum HtreeNodeFindOneError<S: Store> {
    #[error("Key error: {0}")]
    Key(crate::HtreeKeyError<S>),

    #[error("Store error: {0}")]
    Store(S::Error),

    #[error("Error unpacking children: {0}")]
    UnpackChildren(#[from] crate::HtreeNodeUnpackChildrenError),
}

#[allow(unreachable_patterns)]
#[allow(clippy::match_wildcard_for_single_variants)]
impl<S: Store> From<crate::HtreeKeyError<S>> for HtreeNodeFindOneError<S> {
    fn from(value: crate::HtreeKeyError<S>) -> Self {
        match value {
            crate::HtreeKeyError::Store(err) => Self::Store(err),
            err => Self::Key(err),
        }
    }
}

impl<S: Store> From<crate::HtreeNodeSelectChildError<S>> for HtreeNodeFindOneError<S> {
    fn from(value: crate::HtreeNodeSelectChildError<S>) -> Self {
        match value {
            super::HtreeNodeSelectChildError::Key(err) => err.into(),
            crate::HtreeNodeSelectChildError::Store(err) => Self::Store(err),
            super::HtreeNodeSelectChildError::UnpackChildren(err) => Self::UnpackChildren(err),
        }
    }
}
