use ps_hkey::Store;

use crate::{HtreeKey, HtreeNode, HtreeValue};

impl<V> HtreeNode<V> {
    /// Inserts key-value pairs into the tree.
    ///
    /// Multiple nodes may be returned if a node split occurs.
    ///
    /// # Arguments
    /// * `items` - Iterator of key-value pair references to insert
    /// * `store` - Persistence layer for storing nodes and resolving tree state
    ///
    /// # Errors
    /// - [`Store`](HtreeNodeInsertManyError::Store) is returned if persistence fails.
    /// - [`Key`](HtreeNodeInsertManyError::Key) is returned if key conversion fails.
    /// - [`Pack`](HtreeNodeInsertManyError::Pack) is returned if value serialization fails.
    /// - [`InsertLeaves`](HtreeNodeInsertManyError::InsertLeaves) is returned if leaf insertion fails.
    pub fn insert_many<'k, 'v, K, I, S>(
        &self,
        items: I,
        store: &S,
    ) -> Result<Vec<Self>, HtreeNodeInsertManyError<V, S>>
    where
        K: HtreeKey + 'k,
        V: HtreeValue + 'v,
        I: IntoIterator<Item = (&'k K, &'v V)>,
        S: Store,
    {
        let leaves: Vec<Self> = items
            .into_iter()
            .map(|(key, value)| Self::from_kvp(key, value, store))
            .collect::<Result<Vec<Self>, crate::HtreeNodeFromKvpError<V, S>>>()?;

        Ok(self.insert_leaves(leaves, store)?)
    }
}

/// Errors encountered during bulk insertion of key-value pairs.
///
/// Consolidates failures during key-value pair creation and tree insertion,
/// with [`Store`] errors elevated to the top level for centralized handling.
///
/// [`Store`]: HtreeNodeInsertManyError::Store
#[derive(thiserror::Error, Debug)]
pub enum HtreeNodeInsertManyError<T, S>
where
    T: HtreeValue,
    S: Store,
{
    #[error(transparent)]
    InsertLeaves(crate::HtreeNodeInsertLeavesError<S>),
    #[error("Key error: {0}")]
    Key(crate::HtreeKeyError<S>),
    #[error("Pack error: {0}")]
    Pack(T::PackError),
    #[error("Store error: {0}")]
    Store(S::Error),
}

impl<T, S> From<crate::HtreeNodeFromKvpError<T, S>> for HtreeNodeInsertManyError<T, S>
where
    T: HtreeValue,
    S: Store,
{
    fn from(err: crate::HtreeNodeFromKvpError<T, S>) -> Self {
        match err {
            crate::HtreeNodeFromKvpError::Store(err) => Self::Store(err),
            crate::HtreeNodeFromKvpError::Key(err) => err.into(),
            crate::HtreeNodeFromKvpError::Pack(err) => Self::Pack(err),
        }
    }
}

impl<T, S> From<crate::HtreeNodeInsertLeavesError<S>> for HtreeNodeInsertManyError<T, S>
where
    T: HtreeValue,
    S: Store,
{
    fn from(value: crate::HtreeNodeInsertLeavesError<S>) -> Self {
        match value {
            crate::HtreeNodeInsertLeavesError::Store(err) => Self::Store(err),
            err => Self::InsertLeaves(err),
        }
    }
}

/// Feature-conditional `From` implementation for [`HtreeKeyError`].
///
/// Suppressions handle variants that become unreachable when feature flags
/// (i.e. `rkyv`, `serde`) are not enabled at compile time.
///
/// [`HtreeKeyError`]: crate::HtreeKeyError
#[allow(unreachable_patterns)]
#[allow(clippy::match_wildcard_for_single_variants)]
impl<T, S> From<crate::HtreeKeyError<S>> for HtreeNodeInsertManyError<T, S>
where
    T: HtreeValue,
    S: Store,
{
    fn from(value: crate::HtreeKeyError<S>) -> Self {
        match value {
            crate::HtreeKeyError::Store(err) => Self::Store(err),
            err => Self::Key(err),
        }
    }
}
