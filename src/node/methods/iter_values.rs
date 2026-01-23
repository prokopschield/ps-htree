use ps_hkey::Store;

use crate::{HtreeNode, HtreeValue};

impl<T: HtreeValue> HtreeNode<T> {
    /// Returns an iterator over all values in the tree, unpacking them on the fly.
    pub fn iter_values<'a, S: Store>(
        &'a self,
        store: &'a S,
    ) -> impl Iterator<Item = Result<T, HtreeNodeIterValuesError<T, S>>> + 'a {
        self.iter_leaves(store).map(move |res| {
            let bytes = res?
                .hkey
                .resolve(store)
                .map_err(HtreeNodeIterValuesError::Store)?;

            T::unpack_from_bytes(bytes, store).map_err(Into::into)
        })
    }
}

#[derive(thiserror::Error, Debug)]
pub enum HtreeNodeIterValuesError<T: HtreeValue, S: Store> {
    #[error("HtreeNode's state is internally corrupted.")]
    CorruptedState,
    #[error("Store error: {0}")]
    Store(S::Error),
    #[error("Unpack error: {0}")]
    Unpack(T::UnpackError),
    #[error(transparent)]
    UnpackChildren(#[from] crate::HtreeNodeUnpackChildrenError),
}

impl<T: HtreeValue, S: Store> From<crate::HtreeNodeIterLeavesError<S>>
    for HtreeNodeIterValuesError<T, S>
{
    fn from(value: crate::HtreeNodeIterLeavesError<S>) -> Self {
        match value {
            crate::HtreeNodeIterLeavesError::Store(err) => Self::Store(err),
            crate::HtreeNodeIterLeavesError::CorruptedState => Self::CorruptedState,
            crate::HtreeNodeIterLeavesError::UnpackChildren(err) => Self::UnpackChildren(err),
        }
    }
}

impl<T: HtreeValue, S: Store> From<crate::HtreeValueUnpackError<T, S>>
    for HtreeNodeIterValuesError<T, S>
{
    fn from(value: crate::HtreeValueUnpackError<T, S>) -> Self {
        match value {
            crate::HtreeValueUnpackError::Store(err) => Self::Store(err),
            crate::HtreeValueUnpackError::Unpack(err) => Self::Unpack(err),
        }
    }
}
