use std::sync::Arc;

use ps_hkey::Hkey;
use ps_uuid::UUID;

use crate::node::inner::HtreeNodeReadonly;

impl Default for HtreeNodeReadonly {
    fn default() -> Self {
        Self {
            height: 0,
            key: UUID::nil(),
            hkey: Hkey::Raw(Arc::default()),
        }
    }
}
