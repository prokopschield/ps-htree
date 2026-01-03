use ps_hkey::Store;

use crate::{HtreeKey, HtreeNode, HtreeValue};

impl<V> HtreeNode<V> {
    /// Updates key-value pairs in the tree.
    ///
    /// Fails if any of the keys do not already exist in the tree.
    /// Multiple nodes may be returned if a node split occurs.
    ///
    /// # Arguments
    /// * `items` - Iterator of key-value pair references to update
    /// * `store` - Persistence layer for storing nodes and resolving tree state
    ///
    /// # Errors
    /// - [`Store`](HtreeNodeUpdateManyError::Store) is returned if persistence fails.
    /// - [`Key`](HtreeNodeUpdateManyError::Key) is returned if key conversion fails.
    /// - [`Pack`](HtreeNodeUpdateManyError::Pack) is returned if value serialization fails.
    /// - [`UpdateLeaves`](HtreeNodeUpdateManyError::UpdateLeaves) is returned if leaf update fails (e.g. key not found).
    pub fn update_many<'k, 'v, K, I, S>(
        &self,
        items: I,
        store: &S,
    ) -> Result<Vec<Self>, HtreeNodeUpdateManyError<V, S>>
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

        Ok(self.update_leaves(leaves, store)?)
    }
}

/// Errors encountered during bulk update of key-value pairs.
///
/// Consolidates failures during key-value pair creation and tree updates,
/// with [`Store`] errors elevated to the top level for centralized handling.
///
/// [`Store`]: HtreeNodeUpdateManyError::Store
#[derive(thiserror::Error, Debug)]
pub enum HtreeNodeUpdateManyError<T, S>
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

impl<T, S> From<crate::HtreeNodeFromKvpError<T, S>> for HtreeNodeUpdateManyError<T, S>
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

impl<T, S> From<crate::HtreeNodeUpdateLeavesError<S>> for HtreeNodeUpdateManyError<T, S>
where
    T: HtreeValue,
    S: Store,
{
    fn from(value: crate::HtreeNodeUpdateLeavesError<S>) -> Self {
        match value {
            crate::HtreeNodeUpdateLeavesError::Store(err) => Self::Store(err),
            err => Self::UpdateLeaves(err),
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
impl<T, S> From<crate::HtreeKeyError<S>> for HtreeNodeUpdateManyError<T, S>
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
