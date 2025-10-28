mod implementations;

use ps_hkey::Hkey;
use ps_uuid::UUID;

#[derive(Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[repr(C)]
pub struct HtreeNodeReadonly {
    pub key: UUID,
    pub height: u8,
    pub hkey: Hkey,
}
