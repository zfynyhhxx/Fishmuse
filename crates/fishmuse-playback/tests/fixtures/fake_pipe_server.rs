#![cfg(windows)]

use std::sync::atomic::{AtomicU64, Ordering};

use fishmuse_playback::foobar::{
    framing::{FrameDecoder, encode_frame},
    protocol::{Envelope, Message, PROTOCOL_VERSION, decode_json},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::windows::named_pipe::{NamedPipeServer, ServerOptions},
};
use uuid::Uuid;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

pub struct FakePipeServer {
    pipe_name: String,
    server: NamedPipeServer,
}

impl FakePipeServer {
    pub fn bind() -> Self {
        let suffix = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let pipe_name = format!(
            r"\\.\pipe\fishmuse-task9-test-{}-{suffix}",
            std::process::id()
        );
        Self::bind_at(pipe_name)
    }

    pub fn bind_at(pipe_name: impl Into<String>) -> Self {
        let pipe_name = pipe_name.into();
        let server = ServerOptions::new()
            .first_pipe_instance(true)
            .create(&pipe_name)
            .expect("create fake named pipe");
        Self { pipe_name, server }
    }

    pub fn pipe_name(&self) -> &str {
        &self.pipe_name
    }

    pub async fn accept(self) -> FakePipePeer {
        self.server
            .connect()
            .await
            .expect("accept fake pipe client");
        FakePipePeer {
            server: self.server,
            decoder: FrameDecoder::default(),
            ready: Vec::new(),
        }
    }
}

pub struct FakePipePeer {
    server: NamedPipeServer,
    decoder: FrameDecoder,
    ready: Vec<Vec<u8>>,
}

impl FakePipePeer {
    pub async fn read(&mut self) -> Envelope {
        loop {
            if let Some(payload) = self.ready.pop() {
                return decode_json(&payload).expect("client envelope is valid");
            }

            let mut buffer = [0_u8; 4096];
            let count = self.server.read(&mut buffer).await.expect("read fake pipe");
            assert_ne!(count, 0, "client disconnected before sending an envelope");
            let mut frames = self
                .decoder
                .push(&buffer[..count])
                .expect("client framing is valid");
            frames.reverse();
            self.ready.extend(frames);
        }
    }

    pub async fn respond(&mut self, request: &Envelope, message: Message) {
        self.write(Envelope {
            protocol_version: PROTOCOL_VERSION,
            message_id: next_uuid(),
            correlation_id: Some(request.message_id),
            sent_at_unix_ms: 1,
            sequence: None,
            message,
        })
        .await;
    }

    pub async fn event(&mut self, sequence: u64, message: Message) {
        self.write(Envelope {
            protocol_version: PROTOCOL_VERSION,
            message_id: next_uuid(),
            correlation_id: None,
            sent_at_unix_ms: 1,
            sequence: Some(sequence),
            message,
        })
        .await;
    }

    #[allow(dead_code)]
    pub async fn wait_for_disconnect(&mut self) {
        let mut buffer = [0_u8; 64];
        loop {
            match self.server.read(&mut buffer).await {
                Ok(0) => return,
                Ok(_) => {}
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::ConnectionReset
                    ) =>
                {
                    return;
                }
                Err(error) => panic!("wait for fake client disconnect: {error}"),
            }
        }
    }

    async fn write(&mut self, envelope: Envelope) {
        let payload = serde_json::to_vec(&envelope).expect("serialize fake envelope");
        let frame = encode_frame(&payload).expect("frame fake envelope");
        self.server
            .write_all(&frame)
            .await
            .expect("write fake pipe");
        self.server.flush().await.expect("flush fake pipe");
    }
}

pub fn next_uuid() -> Uuid {
    Uuid::from_u128(u128::from(NEXT_ID.fetch_add(1, Ordering::Relaxed)))
}
