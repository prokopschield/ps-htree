use ps_uuid::UUID;

#[derive(Clone, Copy, Debug, Default, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[repr(C)]
pub struct HtreeNodeReadonly {
    pub key: UUID,
    pub height: u8,
}
