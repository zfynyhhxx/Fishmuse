use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};

use fishmuse_domain::{AppError, AppResult, ErrorCategory, ErrorCode, TrackId, UserId};
use lofty::{
    file::TaggedFileExt,
    picture::{MimeType, Picture, PictureType},
};
use sqlx::SqlitePool;

pub const MAX_ARTWORK_BYTES: usize = 5 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Artwork {
    pub mime_type: String,
    pub bytes: Vec<u8>,
}

#[derive(Clone)]
pub struct ArtworkResolver {
    pool: SqlitePool,
}

impl ArtworkResolver {
    #[must_use]
    pub const fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn resolve(&self, user_id: UserId, track_id: TrackId) -> AppResult<Option<Artwork>> {
        let path = self.resolve_media_path(user_id, track_id).await?;
        let Some(path) = path else {
            return Ok(None);
        };
        tokio::task::spawn_blocking(move || resolve_from_path(&path))
            .await
            .map_err(|error| artwork_error(ErrorCode::Internal, error))
    }

    async fn resolve_media_path(
        &self,
        user_id: UserId,
        track_id: TrackId,
    ) -> AppResult<Option<PathBuf>> {
        let bytes = sqlx::query_scalar::<_, Vec<u8>>(
            "SELECT original_path FROM media_assets WHERE user_id = ? AND track_id = ? AND availability = 'available' ORDER BY media_asset_id LIMIT 1",
        )
        .bind(user_id.as_uuid().to_string())
        .bind(track_id.as_uuid().to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| artwork_error(ErrorCode::StorageFailure, error))?;
        bytes.map(|bytes| decode_native_path(&bytes)).transpose()
    }
}

fn resolve_from_path(media_path: &Path) -> Option<Artwork> {
    embedded_artwork(media_path).or_else(|| directory_artwork(media_path))
}

fn embedded_artwork(media_path: &Path) -> Option<Artwork> {
    let tagged = lofty::read_from_path(media_path).ok()?;
    let mut pictures = tagged
        .tags()
        .iter()
        .flat_map(|tag| tag.pictures())
        .collect::<Vec<_>>();
    pictures.sort_by_key(|picture| u8::from(picture.pic_type() != PictureType::CoverFront));
    pictures.into_iter().find_map(artwork_from_picture)
}

fn artwork_from_picture(picture: &Picture) -> Option<Artwork> {
    if picture.data().len() > MAX_ARTWORK_BYTES {
        return None;
    }
    let detected = detect_mime(picture.data())?;
    let declared = picture.mime_type().and_then(allowed_lofty_mime);
    if picture.mime_type().is_some() && declared.as_deref() != Some(detected) {
        return None;
    }
    Some(Artwork {
        mime_type: detected.to_owned(),
        bytes: picture.data().to_vec(),
    })
}

fn allowed_lofty_mime(mime: &MimeType) -> Option<String> {
    let value = mime.as_str().to_ascii_lowercase();
    match value.as_str() {
        "image/jpeg" | "image/jpg" => Some("image/jpeg".to_owned()),
        "image/png" => Some("image/png".to_owned()),
        "image/webp" => Some("image/webp".to_owned()),
        _ => None,
    }
}

fn directory_artwork(media_path: &Path) -> Option<Artwork> {
    let directory = media_path.parent()?;
    let mut candidates = fs::read_dir(directory)
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let file_type = entry.file_type().ok()?;
            if !file_type.is_file() {
                return None;
            }
            let path = entry.path();
            let stem = path.file_stem()?.to_string_lossy().to_ascii_lowercase();
            let priority = match stem.as_str() {
                "cover" => 0,
                "folder" => 1,
                "front" => 2,
                _ => return None,
            };
            let extension = path.extension()?.to_string_lossy().to_ascii_lowercase();
            if !matches!(extension.as_str(), "jpg" | "jpeg" | "png" | "webp") {
                return None;
            }
            Some((
                priority,
                path.file_name()?.to_string_lossy().to_ascii_lowercase(),
                path,
            ))
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| (left.0, &left.1).cmp(&(right.0, &right.1)));
    candidates
        .into_iter()
        .find_map(|(_, _, path)| read_directory_candidate(&path))
}

fn read_directory_candidate(path: &Path) -> Option<Artwork> {
    let length = usize::try_from(fs::metadata(path).ok()?.len()).ok()?;
    if length > MAX_ARTWORK_BYTES {
        return None;
    }
    let bytes = fs::read(path).ok()?;
    let mime_type = detect_mime(&bytes)?.to_owned();
    Some(Artwork { mime_type, bytes })
}

fn detect_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

#[cfg(windows)]
fn decode_native_path(bytes: &[u8]) -> AppResult<PathBuf> {
    use std::os::windows::ffi::OsStringExt;

    if !bytes.len().is_multiple_of(2) {
        return Err(artwork_error(
            ErrorCode::StorageFailure,
            "stored Windows path has invalid byte length",
        ));
    }
    let (pairs, remainder) = bytes.as_chunks::<2>();
    debug_assert!(remainder.is_empty());
    let wide = pairs
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
        .collect::<Vec<_>>();
    Ok(PathBuf::from(OsString::from_wide(&wide)))
}

#[cfg(unix)]
fn decode_native_path(bytes: &[u8]) -> AppResult<PathBuf> {
    use std::os::unix::ffi::OsStringExt;

    Ok(PathBuf::from(OsString::from_vec(bytes.to_vec())))
}

#[cfg(not(any(unix, windows)))]
fn decode_native_path(bytes: &[u8]) -> AppResult<PathBuf> {
    String::from_utf8(bytes.to_vec())
        .map(PathBuf::from)
        .map_err(|error| artwork_error(ErrorCode::StorageFailure, error))
}

fn artwork_error(code: ErrorCode, context: impl std::fmt::Display) -> AppError {
    AppError {
        code,
        category: ErrorCategory::Library,
        user_message: "Artwork is unavailable.".to_owned(),
        retryable: false,
        suggested_action: None,
        technical_context: Some(context.to_string()),
    }
}
