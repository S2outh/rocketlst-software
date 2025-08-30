use defmt::{info, Format};
use embassy_stm32::can::{BufferedCanReceiver, Frame, enums::BusError};
use embedded_can::Id;
use heapless::{FnvIndexMap, Vec};

/// Can frame for the RODOS can protocol
/// conatining the topic and data
pub struct RodosCanFrame<'a> {
    pub(super) topic: u16,
    pub(super) device: u8,
    pub(super) data: &'a [u8],
}

impl<'a> RodosCanFrame<'a> {
    pub fn topic(&self) -> u16 {
        self.topic
    }
    pub fn device(&self) -> u8 {
        self.device
    }
    pub fn data(&self) -> &'a [u8] {
        self.data
    }
}

/// Error enum for can frame decode errors
#[derive(Format)]
pub enum RodosCanDecodeError {
    WrongIDType,
    NoData,
}

/// Error enum for the all RODOS can receiving operations
#[derive(Format)]
pub enum RodosCanReceiveError {
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

struct RodosCanFramePart {
    id: u32,
    data: Vec<u8, 5>,
    seq_num: usize,
    seq_len: usize,
}

/// Module to send messages on a rodos can
pub struct RodosCanReceiver<const NUMBER_OF_SOURCES: usize, const MAX_PACKET_LENGTH: usize> {
    receiver: BufferedCanReceiver,
    partial_frames: FnvIndexMap<u32, Vec<u8, MAX_PACKET_LENGTH>, NUMBER_OF_SOURCES>,
}

impl<const NUMBER_OF_SOURCES: usize, const MAX_PACKET_LENGTH: usize>
    RodosCanReceiver<NUMBER_OF_SOURCES, MAX_PACKET_LENGTH>
{
    /// create a new instance from BufferedCanReceiver
    pub(super) fn new(receiver: BufferedCanReceiver) -> Self {
        RodosCanReceiver {
            receiver,
            partial_frames: FnvIndexMap::new(),
        }
    }
    /// take a u32 extended id and decode it to RODOS id parts
    fn decode_id(id: u32) -> (u16, u8) {
        let topic = (id >> 8) as u16;
        let device = id as u8;
        (topic, device)
    }
    /// take a can hal frame and decode it to RODOS message parts
    fn decode(frame: &Frame) -> Result<RodosCanFramePart, RodosCanDecodeError> {
        let Id::Extended(id) = frame.id() else {
            return Err(RodosCanDecodeError::WrongIDType);
        };
        let id = id.as_raw();

        if frame.data().len() <= 3 {
            // No data in can msg
            return Err(RodosCanDecodeError::NoData);
        }
        let seq_num = frame.data()[0] as usize;
        let seq_len = frame.data()[2] as usize;
        let data = frame.data()[3..].try_into().unwrap();

        Ok(RodosCanFramePart {
            id,
            data,
            seq_num,
            seq_len,
        })
    }
    /// receive the next rodos frame async
    pub async fn receive(&mut self) -> Result<RodosCanFrame, RodosCanReceiveError> {
        loop {
            match self.receiver.receive().await {
                Ok(envelope) => {
                    info!("test");
                    let frame_part = Self::decode(&envelope.frame)
                        .map_err(|e| RodosCanReceiveError::CouldNotDecode(e))?;
                    // check if seq len is too long
                    if frame_part.seq_len * 5 > MAX_PACKET_LENGTH {
                        return Err(RodosCanReceiveError::MessageBufferFull);
                    }
                    // add entry if it doesn't already exist
                    if !self.partial_frames.contains_key(&frame_part.id) {
                        self.partial_frames
                            .insert(frame_part.id, Vec::new())
                            .map_err(|_| RodosCanReceiveError::SourceBufferFull)?;
                    }
                    // if the seq_num is 0 this is the start of a new message. clear the buffer.
                    else if frame_part.seq_num == 0 {
                        self.partial_frames[&frame_part.id] = Vec::new();
                    }
                    let current_seq_num = self.partial_frames[&frame_part.id].len() / 5;
                    // add current frame to buffer
                    if frame_part.seq_num == current_seq_num {
                        self.partial_frames[&frame_part.id].extend(frame_part.data);
                    }
                    // if the seq_num is smaller than the length, this is a dupplicate msg. drop it.
                    else if frame_part.seq_num < current_seq_num {
                        continue;
                    }
                    // if the seq_num does not match the length return an error
                    else {
                        self.partial_frames[&frame_part.id] = Vec::new();
                        return Err(RodosCanReceiveError::FrameDropped);
                    }
                    // if buffer length >= seqence length, the frame is complete.
                    // return the frame and clear the buffer
                    if frame_part.seq_num >= frame_part.seq_len {
                        let data = &self.partial_frames[&frame_part.id][..];
                        let (topic, device) = Self::decode_id(frame_part.id);
                        return Ok(RodosCanFrame {
                            topic,
                            device,
                            data,
                        });
                    }
                }
                Err(e) => return Err(RodosCanReceiveError::BusError(e)),
            }
        }
    }
}
