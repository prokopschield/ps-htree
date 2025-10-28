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
        let packed = value.pack(store).map_err(HtreeNodeFromKvpError::Pack)?;
        let hkey = store.put(&packed).map_err(HtreeNodeFromKvpError::Store)?;

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
    Key(#[from] HtreeKeyError<S>),
    #[error("Pack error: {0}")]
    Pack(T::PackError),
    #[error("Store error: {0}")]
    Store(S::Error),
}
