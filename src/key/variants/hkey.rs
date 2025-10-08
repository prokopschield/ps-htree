use ps_hkey::{Hkey, Store};
use ps_uuid::UUID;

use crate::{HtreeKey, HtreeKeyError, utils::compact_to_uuid};

impl HtreeKey for Hkey {
    fn try_to_hkey<S: Store>(&self, _: &S) -> Result<Hkey, HtreeKeyError<S>> {
        Ok(self.clone())
    }

    #[allow(clippy::cast_possible_truncation)]
    fn try_to_uuid<S: Store>(&self, store: &S) -> Result<UUID, HtreeKeyError<S>> {
        let input = self.compact(store).map_err(HtreeKeyError::Store)?;

        Ok(compact_to_uuid(&input))
    }
}
