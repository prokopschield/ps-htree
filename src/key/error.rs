use ps_hkey::Store;

#[derive(thiserror::Error, Debug)]
pub enum HtreeKeyError<S: Store> {
    #[error("Hkey construction error: {0}")]
    HkeyConstruction(#[from] ps_hkey::HkeyConstructionError),
    #[cfg(feature = "rkyv")]
    #[error(transparent)]
    Rkyv(#[from] rkyv::rancor::Error),
    #[cfg(feature = "serde")]
    #[error("Serialization error: {0}")]
    Ser(#[from] postcard::Error),
    #[error("Store error: {0}")]
    Store(S::Error),
}
