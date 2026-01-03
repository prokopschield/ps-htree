use std::convert::Infallible;

use ps_hkey::Store;

use crate::{HtreeNode, HtreeValue, HtreeValuePackError, HtreeValueUnpackError};

impl<T: HtreeValue> HtreeValue for HtreeNode<T> {
    type PackError = Infallible;
    type UnpackError = HtreeNodeAsValueUnpackError;

    fn pack_owned<S: Store>(
        &self,
        store: &S,
    ) -> Result<bytes::Bytes, HtreeValuePackError<Self, S>> {
        match self.hkey.pack_owned(store) {
            Ok(value) => Ok(value),
            Err(HtreeValuePackError::Pack(err)) => Err(HtreeValuePackError::Pack(err)),
            Err(HtreeValuePackError::Store(err)) => Err(HtreeValuePackError::Store(err)),
        }
    }

    fn pack_into<F, R, S>(&self, closure: F, store: &S) -> Result<R, HtreeValuePackError<Self, S>>
    where
        F: FnOnce(&[u8]) -> R,
        S: Store,
    {
        match self.hkey.pack_into(closure, store) {
            Ok(value) => Ok(value),
            Err(HtreeValuePackError::Pack(err)) => Err(HtreeValuePackError::Pack(err)),
            Err(HtreeValuePackError::Store(err)) => Err(HtreeValuePackError::Store(err)),
        }
    }

    fn unpack<S: Store>(bytes: &[u8], store: &S) -> Result<Self, HtreeValueUnpackError<Self, S>> {
        Self::unpack(bytes, store).map_err(Into::into)
    }
}

/// This type 1:1 corresponds to [`crate::HtreeNodeUnpackError`].
#[derive(thiserror::Error, Debug)]
pub enum HtreeNodeAsValueUnpackError {
    #[error("Child nodes must all have the same height.")]
    ChildHeightInconsistent,
    #[error("Maximum height of 255 exceeded.")]
    HeightOverflow,
    #[error("Integer conversion error: {0}")]
    IntConv(#[from] std::num::TryFromIntError),
    #[error(transparent)]
    UnpackChildren(#[from] crate::HtreeNodeUnpackChildrenError),
}

impl<T, S> From<crate::HtreeNodeUnpackError<S>> for HtreeValueUnpackError<HtreeNode<T>, S>
where
    T: HtreeValue,
    S: Store,
{
    fn from(value: crate::HtreeNodeUnpackError<S>) -> Self {
        match value {
            crate::HtreeNodeUnpackError::FromChildren(err) => err.into(),
            crate::HtreeNodeUnpackError::Store(err) => Self::Store(err),
            crate::HtreeNodeUnpackError::UnpackChildren(err) => {
                Self::Unpack(HtreeNodeAsValueUnpackError::UnpackChildren(err))
            }
        }
    }
}

impl<T, S> From<crate::HtreeNodeFromChildrenError<S>> for HtreeValueUnpackError<HtreeNode<T>, S>
where
    T: HtreeValue,
    S: Store,
{
    fn from(value: crate::HtreeNodeFromChildrenError<S>) -> Self {
        match value {
            crate::HtreeNodeFromChildrenError::ChildHeightInconsistent => {
                Self::Unpack(HtreeNodeAsValueUnpackError::ChildHeightInconsistent)
            }
            crate::HtreeNodeFromChildrenError::HeightOverflow => {
                Self::Unpack(HtreeNodeAsValueUnpackError::HeightOverflow)
            }
            crate::HtreeNodeFromChildrenError::IntConv(err) => {
                Self::Unpack(HtreeNodeAsValueUnpackError::IntConv(err))
            }
            crate::HtreeNodeFromChildrenError::Store(err) => Self::Store(err),
        }
    }
}

impl<S> From<HtreeNodeAsValueUnpackError> for crate::HtreeNodeUnpackError<S>
where
    S: Store,
{
    fn from(value: HtreeNodeAsValueUnpackError) -> Self {
        match value {
            HtreeNodeAsValueUnpackError::ChildHeightInconsistent => {
                Self::FromChildren(crate::HtreeNodeFromChildrenError::ChildHeightInconsistent)
            }
            HtreeNodeAsValueUnpackError::HeightOverflow => {
                Self::FromChildren(crate::HtreeNodeFromChildrenError::HeightOverflow)
            }
            HtreeNodeAsValueUnpackError::IntConv(err) => {
                Self::FromChildren(crate::HtreeNodeFromChildrenError::IntConv(err))
            }
            HtreeNodeAsValueUnpackError::UnpackChildren(err) => Self::UnpackChildren(err),
        }
    }
}

impl<T, S> From<HtreeValueUnpackError<T, S>> for crate::HtreeNodeUnpackError<S>
where
    T: HtreeValue,
    S: Store,
    Self: From<<T as HtreeValue>::UnpackError>,
{
    fn from(value: HtreeValueUnpackError<T, S>) -> Self {
        match value {
            HtreeValueUnpackError::Store(err) => Self::Store(err),
            HtreeValueUnpackError::Unpack(err) => err.into(),
        }
    }
}
