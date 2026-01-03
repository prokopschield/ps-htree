use ps_hkey::Store;
use ps_rwt::RWT;

use crate::{
    HtreeKey, HtreeKeyError, HtreeNode, HtreeValue, LEAF_HEIGHT,
    node::inner::{HtreeNodeReadonly, HtreeNodeWritable},
};

impl<T: HtreeValue> HtreeNode<T> {
    /// Constructs a leaf node from a key-value pair.
    ///
    /// Converts the key to a UUID, packs the value, stores the packed data,
    /// and returns a new leaf node containing the stored reference.
    ///
    /// # Arguments
    ///
    /// * `key` - An object implementing `HtreeKey` to be converted to a UUID.
    /// * `value` - The value to pack and store.
    /// * `store` - The store used for UUID conversion, packing, and storage.
    ///
    /// # Errors
    ///
    /// Returns an error if key conversion, value packing, or storage fails.
    pub fn from_kvp<S: Store>(
        key: &impl HtreeKey,
        value: &T,
        store: &S,
    ) -> Result<Self, HtreeNodeFromKvpError<S, T>> {
        let key = key.try_to_uuid(store)?;
        let hkey = value
            .pack_into(|bytes| store.put(bytes), store)?
            .map_err(HtreeNodeFromKvpError::Store)?;

        Ok(Self {
            inner: RWT::new(
                HtreeNodeReadonly {
                    height: LEAF_HEIGHT,
                    key,
                    hkey,
                },
                HtreeNodeWritable::Leaf,
            ),
        })
    }
}

#[derive(thiserror::Error, Debug)]
pub enum HtreeNodeFromKvpError<S: Store, T: HtreeValue> {
    #[error("Key error: {0}")]
    Key(HtreeKeyError<S>),
    #[error("Pack error: {0}")]
    Pack(T::PackError),
    #[error("Store error: {0}")]
    Store(S::Error),
}

#[allow(unreachable_patterns)]
#[allow(clippy::match_wildcard_for_single_variants)]
impl<S: Store, T: HtreeValue> From<HtreeKeyError<S>> for HtreeNodeFromKvpError<S, T> {
    fn from(value: HtreeKeyError<S>) -> Self {
        match value {
            HtreeKeyError::Store(err) => Self::Store(err),
            err => Self::Key(err),
        }
    }
}

impl<S: Store, T: HtreeValue> From<crate::HtreeValuePackError<T, S>>
    for HtreeNodeFromKvpError<S, T>
{
    fn from(value: crate::HtreeValuePackError<T, S>) -> Self {
        match value {
            crate::HtreeValuePackError::Pack(err) => Self::Pack(err),
            crate::HtreeValuePackError::Store(err) => Self::Store(err),
        }
    }
}
