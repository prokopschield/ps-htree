use ps_hkey::Store;

use crate::{HtreeKey, HtreeNode, HtreeValue};

impl<V> HtreeNode<V> {
    /// Upserts key-value pairs into the tree.
    ///
    /// Multiple nodes may be returned if a node split occurs.
    ///
    /// # Arguments
    /// * `items` - Iterator of key-value pair references to upsert
    /// * `store` - Persistence layer for storing nodes and resolving tree state
    ///
    /// # Errors
    /// - [`Store`](HtreeNodeUpsertManyError::Store) is returned if persistence fails.
    /// - [`Key`](HtreeNodeUpsertManyError::Key) is returned if key conversion fails.
    /// - [`Pack`](HtreeNodeUpsertManyError::Pack) is returned if value serialization fails.
    /// - [`UpsertLeaves`](HtreeNodeUpsertManyError::UpsertLeaves) is returned if leaf upsertion fails.
    pub fn upsert_many<'k, 'v, K, I, S>(
        &self,
        items: I,
        store: &S,
    ) -> Result<Vec<Self>, HtreeNodeUpsertManyError<V, S>>
    where
        K: HtreeKey + ?Sized + 'k,
        V: HtreeValue + 'v,
        I: IntoIterator<Item = (&'k K, &'v V)>,
        S: Store,
    {
        let leaves: Vec<Self> = items
            .into_iter()
            .map(|(key, value)| Self::from_kvp(key, value, store))
            .collect::<Result<Vec<Self>, crate::HtreeNodeFromKvpError<V, S>>>()?;

        Ok(self.upsert_leaves(leaves, store)?)
    }
}

/// Errors encountered during bulk upsertion of key-value pairs.
///
/// Consolidates failures during key-value pair creation and tree upsertion,
/// with [`Store`] errors elevated to the top level for centralized handling.
///
/// [`Store`]: HtreeNodeUpsertManyError::Store
#[derive(thiserror::Error, Debug)]
pub enum HtreeNodeUpsertManyError<T, S>
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

impl<T, S> From<crate::HtreeNodeFromKvpError<T, S>> for HtreeNodeUpsertManyError<T, S>
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

impl<T, S> From<crate::HtreeNodeUpsertLeavesError<S>> for HtreeNodeUpsertManyError<T, S>
where
    T: HtreeValue,
    S: Store,
{
    fn from(value: crate::HtreeNodeUpsertLeavesError<S>) -> Self {
        match value {
            crate::HtreeNodeUpsertLeavesError::Store(err) => Self::Store(err),
            err => Self::UpsertLeaves(err),
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
impl<T, S> From<crate::HtreeKeyError<S>> for HtreeNodeUpsertManyError<T, S>
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
