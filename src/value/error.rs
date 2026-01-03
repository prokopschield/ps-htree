use ps_hkey::Store;

use crate::HtreeValue;

#[derive(thiserror::Error, Debug)]
pub enum HtreeValuePackError<T, S>
where
    T: HtreeValue,
    S: Store,
{
    #[error("Pack error: $0")]
    Pack(T::PackError),
    #[error("Storage error: $0")]
    Store(S::Error),
}

#[derive(thiserror::Error, Debug)]
pub enum HtreeValueUnpackError<T, S>
where
    T: HtreeValue,
    S: Store,
{
    #[error("Unpack error: $0")]
    Unpack(T::UnpackError),
    #[error("Fetch error: $0")]
    Store(S::Error),
}
