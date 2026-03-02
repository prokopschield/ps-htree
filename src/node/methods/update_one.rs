use ps_hkey::Store;

use crate::{HtreeKey, HtreeNode, HtreeValue};

impl<T: HtreeValue> HtreeNode<T> {
    /// Updates a single key-value pair in the tree.
    ///
    /// Equivalent to [`update_many`](Self::update_many) with one item.
    /// Fails if the key does not already exist.
    /// Returns potentially multiple sibling nodes if a tree split occurs.
    ///
    /// # Arguments
    /// * `key` - Key reference to update
    /// * `value` - Value reference to update
    /// * `store` - Persistence layer
    ///
    /// # Errors
    /// - [`Store`](HtreeNodeUpdateOneError::Store) if persistence fails.
    /// - [`Key`](HtreeNodeUpdateOneError::Key) if key conversion fails.
    /// - [`Pack`](HtreeNodeUpdateOneError::Pack) if value serialization fails.
    /// - [`UpdateLeaves`](HtreeNodeUpdateOneError::UpdateLeaves) if update fails (e.g. key not found).
    pub fn update_one<K: HtreeKey + ?Sized, S: Store>(
        &self,
        key: &K,
        value: &T,
        store: &S,
    ) -> Result<Vec<Self>, HtreeNodeUpdateOneError<T, S>> {
        Ok(self.update_many([(key, value)], store)?)
    }
}

#[derive(thiserror::Error, Debug)]
pub enum HtreeNodeUpdateOneError<T, S>
where
    T: HtreeValue,
    S: Store,
{
    #[error(transparent)]
    UpdateLeaves(crate::HtreeNodeUpdateLeavesError<S>),
    #[error("Key error: {0}")]
    Key(crate::HtreeKeyError<S>),
    #[error("Pack error: {0}")]
    Pack(T::PackError),
    #[error("Store error: {0}")]
    Store(S::Error),
}

impl<T, S> From<crate::HtreeNodeUpdateManyError<T, S>> for HtreeNodeUpdateOneError<T, S>
where
    T: HtreeValue,
    S: Store,
{
    fn from(value: crate::HtreeNodeUpdateManyError<T, S>) -> Self {
        match value {
            crate::HtreeNodeUpdateManyError::UpdateLeaves(err) => Self::UpdateLeaves(err),
            crate::HtreeNodeUpdateManyError::Key(err) => Self::Key(err),
            crate::HtreeNodeUpdateManyError::Pack(err) => Self::Pack(err),
            crate::HtreeNodeUpdateManyError::Store(err) => Self::Store(err),
        }
    }
}
