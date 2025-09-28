use bytes::Bytes;
use ps_hkey::Store;
use serde::{Serialize, de::DeserializeOwned};

use crate::HtreeValue;

pub struct HtreeSerdeValue<T>(pub T)
where
    T: Serialize;

impl<T> HtreeValue for HtreeSerdeValue<T>
where
    T: Serialize + DeserializeOwned,
{
    type PackError = ciborium::ser::Error<std::io::Error>;
    type UnpackError = ciborium::de::Error<std::io::Error>;

    fn pack(&self, _: impl Store) -> Result<bytes::Bytes, Self::PackError> {
        let mut bytes = Vec::new();

        ciborium::into_writer(&self.0, &mut bytes)?;

        Ok(Bytes::from_owner(bytes))
    }

    fn unpack(bytes: Bytes, _: impl Store) -> Result<Self, Self::UnpackError> {
        Ok(Self(ciborium::from_reader(&bytes[..])?))
    }
}
