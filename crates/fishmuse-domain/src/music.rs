use std::num::NonZeroU32;

use serde::{Deserialize, Serialize};

use crate::{MediaAssetId, RecordingId, ReleaseId, TrackId};

macro_rules! one_based_number {
    ($name:ident) => {
        #[derive(
            Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize,
        )]
        #[serde(transparent)]
        pub struct $name(NonZeroU32);

        impl $name {
            #[must_use]
            pub const fn new(value: u32) -> Option<Self> {
                match NonZeroU32::new(value) {
                    Some(value) => Some(Self(value)),
                    None => None,
                }
            }

            #[must_use]
            pub const fn get(self) -> u32 {
                self.0.get()
            }
        }
    };
}

one_based_number!(DiscNumber);
one_based_number!(TrackNumber);

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TrackSummary {
    pub id: TrackId,
    pub recording_id: RecordingId,
    pub title: String,
    pub artist_names: Vec<String>,
    pub release_title: Option<String>,
    pub duration_ms: Option<u64>,
    pub disc_number: Option<DiscNumber>,
    pub track_number: Option<TrackNumber>,
    pub playable: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseSummary {
    pub id: ReleaseId,
    pub title: String,
    pub artist_names: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LibraryItem {
    pub track: TrackSummary,
    pub release: Option<ReleaseSummary>,
    pub provenance: Vec<SourceProvenance>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceProvenance {
    Local,
    MusicBrainz,
    AppleMusic,
    NetEase,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PlayableSource {
    pub track_id: TrackId,
    pub media_asset_id: MediaAssetId,
    pub subsong_index: Option<u32>,
    pub start_ms: Option<u64>,
    pub end_ms: Option<u64>,
}
