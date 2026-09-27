use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticCode {
    Unreadable,
    UnsupportedFormat,
    InvalidTags,
    UnsupportedCue,
}

impl DiagnosticCode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unreadable => "unreadable",
            Self::UnsupportedFormat => "unsupported_format",
            Self::InvalidTags => "invalid_tags",
            Self::UnsupportedCue => "unsupported_cue",
        }
    }
}
