use std::fmt;

use serde::{Deserialize, Deserializer, Serialize};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidDomainId;

impl fmt::Display for InvalidDomainId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("domain IDs must use UUID v7")
    }
}

impl std::error::Error for InvalidDomainId {}

macro_rules! domain_id {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            #[must_use]
            #[allow(clippy::new_without_default)]
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }

            pub fn try_from_uuid(uuid: Uuid) -> Result<Self, InvalidDomainId> {
                if uuid.get_version_num() == 7 {
                    Ok(Self(uuid))
                } else {
                    Err(InvalidDomainId)
                }
            }

            #[must_use]
            pub const fn as_uuid(self) -> Uuid {
                self.0
            }
        }

        impl TryFrom<Uuid> for $name {
            type Error = InvalidDomainId;

            fn try_from(value: Uuid) -> Result<Self, Self::Error> {
                Self::try_from_uuid(value)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let uuid = Uuid::deserialize(deserializer)?;
                Self::try_from_uuid(uuid).map_err(serde::de::Error::custom)
            }
        }
    };
}

domain_id!(UserId);
domain_id!(ArtistId);
domain_id!(ReleaseId);
domain_id!(RecordingId);
domain_id!(TrackId);
domain_id!(MediaAssetId);
domain_id!(ListenId);
domain_id!(ConversationId);
domain_id!(OperationId);
domain_id!(ScanId);
