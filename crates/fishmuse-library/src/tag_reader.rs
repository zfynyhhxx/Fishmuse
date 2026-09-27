use std::path::{Path, PathBuf};

use async_trait::async_trait;
use lofty::{
    error::ErrorKind,
    prelude::{Accessor, AudioFile, TaggedFileExt},
};
use serde::{Deserialize, Serialize};

use crate::DiagnosticCode;

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ParsedTags {
    pub title: Option<String>,
    pub artists: Vec<String>,
    pub album: Option<String>,
    pub duration_ms: Option<u64>,
    pub disc_number: Option<u32>,
    pub track_number: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScanFailure {
    pub code: DiagnosticCode,
    pub message: String,
    #[serde(skip_serializing, skip_deserializing)]
    pub technical_context: Option<String>,
}

impl ScanFailure {
    #[must_use]
    pub fn new(code: DiagnosticCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            technical_context: None,
        }
    }

    fn with_context(mut self, context: impl std::fmt::Display) -> Self {
        self.technical_context = Some(context.to_string());
        self
    }
}

#[async_trait]
pub trait TagReader: Send + Sync {
    async fn read(&self, path: &Path) -> Result<ParsedTags, ScanFailure>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct LoftyTagReader;

#[async_trait]
impl TagReader for LoftyTagReader {
    async fn read(&self, path: &Path) -> Result<ParsedTags, ScanFailure> {
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || read_tags(path))
            .await
            .map_err(|error| {
                ScanFailure::new(DiagnosticCode::InvalidTags, "tag parser task failed")
                    .with_context(error)
            })?
    }
}

fn read_tags(path: PathBuf) -> Result<ParsedTags, ScanFailure> {
    let tagged_file = lofty::read_from_path(&path).map_err(map_lofty_error)?;
    let duration_ms = u64::try_from(tagged_file.properties().duration().as_millis()).ok();
    let tag = tagged_file
        .primary_tag()
        .or_else(|| tagged_file.first_tag());
    Ok(match tag {
        Some(tag) => ParsedTags {
            title: tag.title().map(|value| value.into_owned()),
            artists: tag
                .artist()
                .map(|value| vec![value.into_owned()])
                .unwrap_or_default(),
            album: tag.album().map(|value| value.into_owned()),
            duration_ms,
            disc_number: tag.disk(),
            track_number: tag.track(),
        },
        None => ParsedTags {
            duration_ms,
            ..ParsedTags::default()
        },
    })
}

fn map_lofty_error(error: lofty::error::LoftyError) -> ScanFailure {
    let code = match error.kind() {
        ErrorKind::UnknownFormat => DiagnosticCode::UnsupportedFormat,
        ErrorKind::Io(_) => DiagnosticCode::Unreadable,
        _ => DiagnosticCode::InvalidTags,
    };
    ScanFailure::new(code, code.as_str()).with_context(error)
}
