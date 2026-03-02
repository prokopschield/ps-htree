use ps_hkey::Store;

use crate::{HtreeKey, HtreeNode, HtreeValue};

impl<T: HtreeValue> HtreeNode<T> {
    /// Upserts a single key-value pair into the tree.
    ///
    /// Equivalent to [`upsert_many`](Self::upsert_many) with one item.
    /// Returns potentially multiple sibling nodes if a tree split occurs.
    ///
    /// # Arguments
    /// * `key` - Key reference to upsert
    /// * `value` - Value reference to upsert
    /// * `store` - Persistence layer
    ///
    /// # Errors
    /// - [`Store`](HtreeNodeUpsertOneError::Store) if persistence fails.
    /// - [`Key`](HtreeNodeUpsertOneError::Key) if key conversion fails.
    /// - [`Pack`](HtreeNodeUpsertOneError::Pack) if value serialization fails.
    /// - [`UpsertLeaves`](HtreeNodeUpsertOneError::UpsertLeaves) if upsertion fails.
    pub fn upsert_one<K: HtreeKey + ?Sized, S: Store>(
        &self,
        key: &K,
        value: &T,
        store: &S,
    ) -> Result<Vec<Self>, HtreeNodeUpsertOneError<T, S>> {
        Ok(self.upsert_many([(key, value)], store)?)
    }
}

#[derive(thiserror::Error, Debug)]
pub enum HtreeNodeUpsertOneError<T, S>
where
    T: HtreeValue,
    S: Store,
{
    #[error(transparent)]
    UpsertLeaves(crate::HtreeNodeUpsertLeavesError<S>),
    #[error("Key error: {0}")]
    Key(crate::HtreeKeyError<S>),
    #[error("Pack error: {0}")]
    Pack(T::PackError),
    #[error("Store error: {0}")]
    Store(S::Error),
}

impl<T, S> From<crate::HtreeNodeUpsertManyError<T, S>> for HtreeNodeUpsertOneError<T, S>
where
    T: HtreeValue,
    S: Store,
{
    fn from(value: crate::HtreeNodeUpsertManyError<T, S>) -> Self {
        match value {
            crate::HtreeNodeUpsertManyError::UpsertLeaves(err) => Self::UpsertLeaves(err),
            crate::HtreeNodeUpsertManyError::Key(err) => Self::Key(err),
            crate::HtreeNodeUpsertManyError::Pack(err) => Self::Pack(err),
            crate::HtreeNodeUpsertManyError::Store(err) => Self::Store(err),
        }
    }
}
