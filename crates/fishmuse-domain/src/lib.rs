#![forbid(unsafe_code)]

mod account;
mod error;
mod ids;
mod listening;
mod music;

pub use account::User;
pub use error::{AppError, AppResult, ErrorCategory, ErrorCode};
pub use ids::{
    ArtistId, ConversationId, InvalidDomainId, ListenId, MediaAssetId, OperationId, RecordingId,
    ReleaseId, TrackId, UserId,
};
pub use listening::ListenSummary;
pub use music::{
    DiscNumber, LibraryItem, PlayableSource, ReleaseSummary, SourceProvenance, TrackNumber,
    TrackSummary,
};
