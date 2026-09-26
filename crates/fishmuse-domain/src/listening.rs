use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::{ListenId, TrackId};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ListenSummary {
    pub id: ListenId,
    pub track_id: TrackId,
    pub started_at: OffsetDateTime,
    pub listened_ms: u64,
    pub completed: bool,
}
