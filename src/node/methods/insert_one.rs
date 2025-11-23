use ps_hkey::Store;

use crate::{HtreeKey, HtreeNode, HtreeValue};

impl<T: HtreeValue> HtreeNode<T> {
    /// Inserts a single key-value pair into the tree.
    ///
    /// Equivalent to [`insert_many`](Self::insert_many) with one item.
    /// Returns potentially multiple sibling nodes if a tree split occurs.
    ///
    /// # Arguments
    /// * `key` - Key reference to insert
    /// * `value` - Value reference to insert
    /// * `store` - Persistence layer
    ///
    /// # Errors
    /// - [`Store`](HtreeNodeInsertOneError::Store) if persistence fails.
    /// - [`Key`](HtreeNodeInsertOneError::Key) if key conversion fails.
    /// - [`Pack`](HtreeNodeInsertOneError::Pack) if value serialization fails.
    /// - [`InsertLeaves`](HtreeNodeInsertOneError::InsertLeaves) if insertion fails.
    pub fn insert_one<S: Store>(
        &self,
        key: &impl HtreeKey,
        value: &T,
        store: &S,
    ) -> Result<Vec<Self>, HtreeNodeInsertOneError<S, T>> {
        Ok(self.insert_many([(key, value)], store)?)
    }
}

#[derive(thiserror::Error, Debug)]
pub enum HtreeNodeInsertOneError<S: Store, V: HtreeValue> {
    #[error(transparent)]
    InsertLeaves(crate::HtreeNodeInsertLeavesError<S>),
    #[error("Key error: {0}")]
    Key(crate::HtreeKeyError<S>),
    #[error("Pack error: {0}")]
    Pack(V::PackError),
    #[error("Store error: {0}")]
    Store(S::Error),
}

impl<S: Store, V: HtreeValue> From<crate::HtreeNodeInsertManyError<S, V>>
    for HtreeNodeInsertOneError<S, V>
{
    fn from(value: crate::HtreeNodeInsertManyError<S, V>) -> Self {
        match value {
            crate::HtreeNodeInsertManyError::InsertLeaves(err) => Self::InsertLeaves(err),
            crate::HtreeNodeInsertManyError::Key(err) => Self::Key(err),
            crate::HtreeNodeInsertManyError::Pack(err) => Self::Pack(err),
            crate::HtreeNodeInsertManyError::Store(err) => Self::Store(err),
        }
    }
}
