use ps_hkey::Hkey;
use ps_uuid::UUID;

use crate::{HtreeKey, HtreeKeyError};

impl HtreeKey for UUID {
    fn try_to_hkey<S: ps_hkey::Store>(&self, _: &S) -> Result<ps_hkey::Hkey, HtreeKeyError<S>> {
        Ok(Hkey::Raw(self.as_bytes().as_slice().into()))
    }

    fn try_to_uuid<S: ps_hkey::Store>(&self, _: &S) -> Result<UUID, HtreeKeyError<S>> {
        Ok(*self)
    }
}

#[cfg(test)]
mod tests {
    use ps_hkey::InMemoryStore;
    use ps_uuid::UUID;

    use crate::{HtreeKey, HtreeKeyError};

    #[test]
    fn identity() -> Result<(), HtreeKeyError<InMemoryStore>> {
        let store = InMemoryStore::default();

        let uuid1 = UUID::gen_v4().with_version(8);
        let hkey1 = uuid1.try_to_hkey(&store)?;
        let uuid2 = hkey1.try_to_uuid(&store)?;
        let hkey2 = uuid2.try_to_hkey(&store)?;

        assert_eq!(uuid1, uuid2, "UUIDs should match.");
        assert_eq!(hkey1, hkey2, "Hkeys should match.");

        Ok(())
    }
}
