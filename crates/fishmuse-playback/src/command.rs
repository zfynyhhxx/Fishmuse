use fishmuse_domain::{OperationId, PlayableSource};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlaybackCommand {
    Play {
        source: PlayableSource,
        operation_id: OperationId,
    },
    Pause {
        operation_id: OperationId,
    },
    Resume {
        operation_id: OperationId,
    },
    Seek {
        position_ms: u64,
        operation_id: OperationId,
    },
    SkipNext {
        operation_id: OperationId,
    },
}

impl PlaybackCommand {
    #[must_use]
    pub const fn operation_id(&self) -> OperationId {
        match self {
            Self::Play { operation_id, .. }
            | Self::Pause { operation_id }
            | Self::Resume { operation_id }
            | Self::Seek { operation_id, .. }
            | Self::SkipNext { operation_id } => *operation_id,
        }
    }

    #[must_use]
    pub fn fingerprint(&self) -> CommandFingerprint {
        let mut hasher = Sha256::new();
        match self {
            Self::Play { source, .. } => {
                hasher.update([0]);
                hasher.update(source.track_id.as_uuid().as_bytes());
                hasher.update(source.media_asset_id.as_uuid().as_bytes());
                hash_option_u64(&mut hasher, source.subsong_index.map(u64::from));
                hash_option_u64(&mut hasher, source.start_ms);
                hash_option_u64(&mut hasher, source.end_ms);
            }
            Self::Pause { .. } => hasher.update([1]),
            Self::Resume { .. } => hasher.update([2]),
            Self::Seek { position_ms, .. } => {
                hasher.update([3]);
                hasher.update(position_ms.to_be_bytes());
            }
            Self::SkipNext { .. } => hasher.update([4]),
        }
        CommandFingerprint(hasher.finalize().into())
    }
}

fn hash_option_u64(hasher: &mut Sha256, value: Option<u64>) {
    match value {
        Some(value) => {
            hasher.update([1]);
            hasher.update(value.to_be_bytes());
        }
        None => hasher.update([0]),
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct CommandFingerprint([u8; 32]);

impl CommandFingerprint {
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    #[must_use]
    pub fn to_hex(self) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut output = String::with_capacity(64);
        for byte in self.0 {
            output.push(char::from(HEX[usize::from(byte >> 4)]));
            output.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
        output
    }
}
