use bytes::Bytes;
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

    fn pack<S>(&self, _store: &S) -> Result<bytes::Bytes, Self::PackError> {
        let mut bytes = Vec::new();

        ciborium::into_writer(&self.0, &mut bytes)?;

        Ok(Bytes::from_owner(bytes))
    }

    fn unpack<S>(bytes: Bytes, _store: &S) -> Result<Self, Self::UnpackError> {
        Ok(Self(ciborium::from_reader(&bytes[..])?))
    }
}
