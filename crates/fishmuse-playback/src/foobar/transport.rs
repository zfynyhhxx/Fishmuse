use std::{collections::VecDeque, fmt, io};

use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadHalf, WriteHalf},
    net::windows::named_pipe::{ClientOptions, NamedPipeClient},
};

use super::{
    framing::{FrameDecoder, encode_frame},
    protocol::{Envelope, ProtocolError, decode_json},
};

type PipeReader = FramedReader<ReadHalf<NamedPipeClient>>;
type PipeWriter = FramedWriter<WriteHalf<NamedPipeClient>>;

#[derive(Debug)]
pub(crate) enum TransportError {
    Io(io::ErrorKind),
    Protocol(ProtocolError),
}

impl fmt::Display for TransportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(kind) => write!(formatter, "pipe I/O failed: {kind:?}"),
            Self::Protocol(error) => write!(formatter, "pipe protocol failed: {error}"),
        }
    }
}

impl std::error::Error for TransportError {}

impl From<io::Error> for TransportError {
    fn from(error: io::Error) -> Self {
        Self::Io(error.kind())
    }
}

impl From<ProtocolError> for TransportError {
    fn from(error: ProtocolError) -> Self {
        Self::Protocol(error)
    }
}

pub(crate) fn connect(pipe_name: &str) -> Result<(PipeReader, PipeWriter), TransportError> {
    let client = ClientOptions::new().open(pipe_name)?;
    let (reader, writer) = tokio::io::split(client);
    Ok((FramedReader::new(reader), FramedWriter::new(writer)))
}

pub(crate) struct FramedReader<R> {
    reader: R,
    decoder: FrameDecoder,
    ready: VecDeque<Vec<u8>>,
}

impl<R> FramedReader<R>
where
    R: AsyncRead + Unpin,
{
    fn new(reader: R) -> Self {
        Self {
            reader,
            decoder: FrameDecoder::default(),
            ready: VecDeque::new(),
        }
    }

    pub(crate) async fn read(&mut self) -> Result<Envelope, TransportError> {
        loop {
            if let Some(payload) = self.ready.pop_front() {
                return Ok(decode_json(&payload)?);
            }

            let mut buffer = [0_u8; 8192];
            let count = self.reader.read(&mut buffer).await?;
            if count == 0 {
                self.decoder.finish()?;
                return Err(TransportError::Io(io::ErrorKind::UnexpectedEof));
            }
            self.ready.extend(self.decoder.push(&buffer[..count])?);
        }
    }
}

pub(crate) struct FramedWriter<W> {
    writer: W,
}

impl<W> FramedWriter<W>
where
    W: AsyncWrite + Unpin,
{
    fn new(writer: W) -> Self {
        Self { writer }
    }

    pub(crate) async fn write(&mut self, envelope: &Envelope) -> Result<(), TransportError> {
        let payload = serde_json::to_vec(envelope)
            .map_err(|error| ProtocolError::invalid(error.to_string()))?;
        let frame = encode_frame(&payload)?;
        self.writer.write_all(&frame).await?;
        self.writer.flush().await?;
        Ok(())
    }
}
