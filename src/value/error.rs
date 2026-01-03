#[derive(thiserror::Error, Debug)]
pub enum HtreeValuePackError<Ser, St> {
    #[error("Pack error: $0")]
    Pack(Ser),
    #[error("Storage error: $0")]
    Store(St),
}

#[derive(thiserror::Error, Debug)]
pub enum HtreeValueUnpackError<D, F> {
    #[error("Unpack error: $0")]
    Unpack(D),
    #[error("Fetch error: $0")]
    Store(F),
}
