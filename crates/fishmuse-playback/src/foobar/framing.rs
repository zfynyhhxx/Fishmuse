use super::protocol::ProtocolError;

pub const MAX_FRAME_SIZE: usize = 1024 * 1024;
const HEADER_SIZE: usize = size_of::<u32>();

pub fn encode_frame(payload: &[u8]) -> Result<Vec<u8>, ProtocolError> {
    if payload.is_empty() {
        return Err(ProtocolError::invalid("frame payload must not be empty"));
    }
    if payload.len() > MAX_FRAME_SIZE {
        return Err(ProtocolError::frame_too_large(format!(
            "frame length {} exceeds {MAX_FRAME_SIZE}",
            payload.len()
        )));
    }

    let length = u32::try_from(payload.len())
        .map_err(|_| ProtocolError::frame_too_large("frame length does not fit u32"))?;
    let mut frame = Vec::with_capacity(HEADER_SIZE + payload.len());
    frame.extend_from_slice(&length.to_le_bytes());
    frame.extend_from_slice(payload);
    Ok(frame)
}

#[derive(Clone, Debug, Default)]
pub struct FrameDecoder {
    buffer: Vec<u8>,
}

impl FrameDecoder {
    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<Vec<u8>>, ProtocolError> {
        self.buffer.extend_from_slice(bytes);
        let mut frames = Vec::new();

        loop {
            if self.buffer.len() < HEADER_SIZE {
                break;
            }
            let length = u32::from_le_bytes(self.buffer[..HEADER_SIZE].try_into().unwrap());
            let length = usize::try_from(length)
                .map_err(|_| ProtocolError::frame_too_large("frame length is not representable"))?;
            if length == 0 {
                self.buffer.clear();
                return Err(ProtocolError::invalid("zero-length frame"));
            }
            if length > MAX_FRAME_SIZE {
                self.buffer.clear();
                return Err(ProtocolError::frame_too_large(format!(
                    "frame length {length} exceeds {MAX_FRAME_SIZE}"
                )));
            }
            let frame_end = HEADER_SIZE + length;
            if self.buffer.len() < frame_end {
                break;
            }
            frames.push(self.buffer[HEADER_SIZE..frame_end].to_vec());
            self.buffer.drain(..frame_end);
        }

        Ok(frames)
    }

    pub fn finish(&self) -> Result<(), ProtocolError> {
        if self.buffer.is_empty() {
            Ok(())
        } else {
            Err(ProtocolError::invalid("truncated frame"))
        }
    }
}
