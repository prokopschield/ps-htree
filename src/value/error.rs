#[derive(thiserror::Error, Debug)]
pub enum HtreeValueStoreError<Ser, St> {
    #[error("Serialization error: $0")]
    Serialization(Ser),
    #[error("Storage error: $0")]
    Storage(St),
}

#[derive(thiserror::Error, Debug)]
pub enum HtreeValueFetchError<D, F> {
    #[error("Deserialization error: $0")]
    Deserialization(D),
    #[error("Fetch error: $0")]
    Fetch(F),
}
