use ps_hkey::Store;

#[derive(thiserror::Error, Debug)]
pub enum HtreeKeyError<S: Store> {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[cfg(feature = "rkyv")]
    #[error(transparent)]
    Rkyv(#[from] rkyv::rancor::Error),
    #[cfg(feature = "serde")]
    #[error("Serialization error: {0}")]
    Ser(String),
    #[error("Store error: {0}")]
    Store(S::Error),
}

#[cfg(feature = "serde")]
impl<S: Store> From<ciborium::ser::Error<std::io::Error>> for HtreeKeyError<S> {
    fn from(value: ciborium::ser::Error<std::io::Error>) -> Self {
        match value {
            ciborium::ser::Error::Io(err) => Self::Io(err),
            ciborium::ser::Error::Value(err) => Self::Ser(err),
        }
    }
}
