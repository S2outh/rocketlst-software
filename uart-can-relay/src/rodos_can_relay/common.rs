use embassy_stm32::can::enums::BusError;

/// Can frame for the RODOS can protocol
/// conatining the topic and data
pub struct RodosCanFrame<'a> {
    pub(super) topic: u16,
    pub(super) device: u8,
    pub(super) data: &'a[u8],
}

impl<'a> RodosCanFrame<'a> {
    pub fn topic(&self) -> u16 { self.topic }
    pub fn device(&self) -> u8 { self.device }
    pub fn data(&self) -> &'a[u8] { self.data }
}

/// Error enum for can frame decode errors
pub enum RodosCanDecodeError {
    WrongIDType,
    NoData,
}

/// Error enum for the all RODOS can related operations
pub enum RodosCanError {
    /// error in the underlying can error
    BusError(BusError),
    /// the can message could not be decoded as RODOS can message
    /// (It is likely not a RODOS can message. make sure not to use dupplicate ids!)
    CouldNotDecode(RodosCanDecodeError),
    /// one of the message frames has been dropped
    FrameDropped,
    /// the map for different sources is full
    SourceBufferFull,
    /// the message buffer for this specific map is full
    MessageBufferFull,
}
