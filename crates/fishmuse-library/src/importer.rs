use fishmuse_domain::{
    AppError, AppResult, ArtistId, ErrorCategory, ErrorCode, MediaAssetId, RecordingId, ReleaseId,
    TrackId, UserId,
};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use time::OffsetDateTime;

use crate::{ParsedTags, canonicalize_tags};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalImport {
    pub media_asset_id: MediaAssetId,
    pub normalized_path: Vec<u8>,
    pub original_path: Vec<u8>,
    pub content_fingerprint: String,
    pub tags: ParsedTags,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImportOutcome {
    pub media_asset_id: MediaAssetId,
    pub track_id: TrackId,
    pub recording_id: RecordingId,
    pub possible_match: bool,
}

#[derive(Clone)]
pub struct LocalLibraryImporter {
    pool: SqlitePool,
    user_id: UserId,
}

#[derive(Serialize, Deserialize)]
struct NormalizedArtists(Vec<String>);

impl LocalLibraryImporter {
    #[must_use]
    pub const fn new(pool: SqlitePool, user_id: UserId) -> Self {
        Self { pool, user_id }
    }

    pub async fn import(&self, input: LocalImport) -> AppResult<ImportOutcome> {
        let canonical = canonicalize_tags(input.tags.clone());
        let normalized_title = canonical.search_title.clone().unwrap_or_default();
        let normalized_artists =
            serde_json::to_string(&NormalizedArtists(canonical.search_artists.clone()))
                .map_err(import_error)?;
        let normalized_release = canonical.search_release.clone();
        let raw_tags_json = serde_json::to_string(&input.tags).map_err(import_error)?;
        let user = self.user_id.as_uuid().to_string();
        let mut transaction = self.pool.begin().await.map_err(import_error)?;

        let existing = sqlx::query(
            "SELECT media_assets.media_asset_id, media_assets.track_id, tracks.recording_id FROM media_assets LEFT JOIN tracks ON tracks.user_id = media_assets.user_id AND tracks.track_id = media_assets.track_id WHERE media_assets.user_id = ? AND (media_assets.normalized_path = ? OR media_assets.media_asset_id = ?) ORDER BY CASE WHEN media_assets.normalized_path = ? THEN 0 ELSE 1 END LIMIT 1",
        )
        .bind(&user)
        .bind(&input.normalized_path)
        .bind(input.media_asset_id.as_uuid().to_string())
        .bind(&input.normalized_path)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(import_error)?;

        let mut unprojected_asset_id = None;
        if let Some(row) = existing {
            let asset_id =
                parse_media_asset_id(row.try_get("media_asset_id").map_err(import_error)?)?;
            let track_id = row
                .try_get::<Option<String>, _>("track_id")
                .map_err(import_error)?
                .map(|value| parse_track_id(&value))
                .transpose()?;
            let recording_id = row
                .try_get::<Option<String>, _>("recording_id")
                .map_err(import_error)?
                .map(|value| parse_recording_id(&value))
                .transpose()?;
            if let (Some(track_id), Some(recording_id)) = (track_id, recording_id) {
                update_asset_and_metadata(
                    &mut transaction,
                    &user,
                    asset_id,
                    &input,
                    &raw_tags_json,
                    &normalized_title,
                    &normalized_artists,
                    normalized_release.as_deref(),
                )
                .await?;
                let possible_match =
                    possible_match_exists(&mut transaction, &user, recording_id).await?;
                transaction.commit().await.map_err(import_error)?;
                return Ok(ImportOutcome {
                    media_asset_id: asset_id,
                    track_id,
                    recording_id,
                    possible_match,
                });
            }
            unprojected_asset_id = Some(asset_id);
        }

        let moved = if unprojected_asset_id.is_none() {
            sqlx::query(
                "SELECT media_assets.media_asset_id, media_assets.track_id, tracks.recording_id FROM media_assets JOIN tracks ON tracks.user_id = media_assets.user_id AND tracks.track_id = media_assets.track_id WHERE media_assets.user_id = ? AND media_assets.availability = 'missing' AND media_assets.content_fingerprint = ? ORDER BY media_assets.media_asset_id LIMIT 1",
            )
            .bind(&user)
            .bind(&input.content_fingerprint)
            .fetch_optional(&mut *transaction)
            .await
            .map_err(import_error)?
        } else {
            None
        };
        if let Some(row) = moved {
            let asset_id =
                parse_media_asset_id(row.try_get("media_asset_id").map_err(import_error)?)?;
            let track_id = parse_track_id(row.try_get("track_id").map_err(import_error)?)?;
            let recording_id =
                parse_recording_id(row.try_get("recording_id").map_err(import_error)?)?;
            update_asset_and_metadata(
                &mut transaction,
                &user,
                asset_id,
                &input,
                &raw_tags_json,
                &normalized_title,
                &normalized_artists,
                normalized_release.as_deref(),
            )
            .await?;
            let possible_match =
                possible_match_exists(&mut transaction, &user, recording_id).await?;
            transaction.commit().await.map_err(import_error)?;
            return Ok(ImportOutcome {
                media_asset_id: asset_id,
                track_id,
                recording_id,
                possible_match,
            });
        }

        let recording_id = RecordingId::new();
        let track_id = TrackId::new();
        let release_id = input.tags.album.as_ref().map(|_| ReleaseId::new());
        let title = input
            .tags
            .title
            .clone()
            .unwrap_or_else(|| "Unknown title".to_owned());
        let duration_ms = input
            .tags
            .duration_ms
            .map(i64::try_from)
            .transpose()
            .map_err(import_error)?;
        let imported_at = OffsetDateTime::now_utc().unix_timestamp();

        sqlx::query("INSERT INTO recordings(recording_id, user_id, title, normalized_title, duration_ms, provenance) VALUES (?, ?, ?, ?, ?, 'local_tags')")
            .bind(recording_id.as_uuid().to_string())
            .bind(&user)
            .bind(&title)
            .bind(&normalized_title)
            .bind(duration_ms)
            .execute(&mut *transaction)
            .await
            .map_err(import_error)?;
        if let (Some(release_id), Some(release_title)) = (release_id, input.tags.album.as_ref()) {
            sqlx::query("INSERT INTO releases(release_id, user_id, title, normalized_title) VALUES (?, ?, ?, ?)")
                .bind(release_id.as_uuid().to_string())
                .bind(&user)
                .bind(release_title)
                .bind(normalized_release.as_deref().unwrap_or_default())
                .execute(&mut *transaction)
                .await
                .map_err(import_error)?;
        }
        sqlx::query("INSERT INTO tracks(track_id, user_id, recording_id, release_id, title, normalized_title, disc_number, track_number, duration_ms, playable, imported_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?)")
            .bind(track_id.as_uuid().to_string())
            .bind(&user)
            .bind(recording_id.as_uuid().to_string())
            .bind(release_id.map(|id| id.as_uuid().to_string()))
            .bind(&title)
            .bind(&normalized_title)
            .bind(input.tags.disc_number.map(i64::from))
            .bind(input.tags.track_number.map(i64::from))
            .bind(duration_ms)
            .bind(imported_at)
            .execute(&mut *transaction)
            .await
            .map_err(import_error)?;
        for (position, (name, normalized_name)) in input
            .tags
            .artists
            .iter()
            .zip(&canonical.search_artists)
            .enumerate()
        {
            let artist_id = ArtistId::new();
            sqlx::query("INSERT INTO artists(artist_id, user_id, name, normalized_name) VALUES (?, ?, ?, ?)")
                .bind(artist_id.as_uuid().to_string())
                .bind(&user)
                .bind(name)
                .bind(normalized_name)
                .execute(&mut *transaction)
                .await
                .map_err(import_error)?;
            sqlx::query("INSERT INTO track_artists(user_id, track_id, artist_id, position) VALUES (?, ?, ?, ?)")
                .bind(&user)
                .bind(track_id.as_uuid().to_string())
                .bind(artist_id.as_uuid().to_string())
                .bind(i64::try_from(position).map_err(import_error)?)
                .execute(&mut *transaction)
                .await
                .map_err(import_error)?;
        }
        let projected_asset_id = unprojected_asset_id.unwrap_or(input.media_asset_id);
        if unprojected_asset_id.is_some() {
            sqlx::query("UPDATE media_assets SET track_id = ?, normalized_path = ?, original_path = ?, content_fingerprint = ?, availability = 'available' WHERE user_id = ? AND media_asset_id = ?")
                .bind(track_id.as_uuid().to_string())
                .bind(&input.normalized_path)
                .bind(&input.original_path)
                .bind(&input.content_fingerprint)
                .bind(&user)
                .bind(projected_asset_id.as_uuid().to_string())
                .execute(&mut *transaction)
                .await
                .map_err(import_error)?;
        } else {
            sqlx::query("INSERT INTO media_assets(media_asset_id, user_id, track_id, normalized_path, original_path, content_fingerprint, availability) VALUES (?, ?, ?, ?, ?, ?, 'available')")
                .bind(projected_asset_id.as_uuid().to_string())
                .bind(&user)
                .bind(track_id.as_uuid().to_string())
                .bind(&input.normalized_path)
                .bind(&input.original_path)
                .bind(&input.content_fingerprint)
                .execute(&mut *transaction)
                .await
                .map_err(import_error)?;
        }
        insert_metadata(
            &mut transaction,
            &user,
            projected_asset_id,
            &raw_tags_json,
            &normalized_title,
            &normalized_artists,
            normalized_release.as_deref(),
        )
        .await?;

        let candidate = sqlx::query_scalar::<_, String>(
            "SELECT recordings.recording_id FROM recordings JOIN tracks ON tracks.user_id = recordings.user_id AND tracks.recording_id = recordings.recording_id JOIN media_assets ON media_assets.user_id = tracks.user_id AND media_assets.track_id = tracks.track_id JOIN local_import_metadata ON local_import_metadata.user_id = media_assets.user_id AND local_import_metadata.media_asset_id = media_assets.media_asset_id WHERE recordings.user_id = ? AND recordings.recording_id <> ? AND recordings.normalized_title = ? AND recordings.duration_ms IS ? AND local_import_metadata.normalized_artists_json = ? ORDER BY recordings.recording_id LIMIT 1",
        )
        .bind(&user)
        .bind(recording_id.as_uuid().to_string())
        .bind(&normalized_title)
        .bind(duration_ms)
        .bind(&normalized_artists)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(import_error)?;
        let possible_match = if let Some(candidate) = candidate {
            sqlx::query("INSERT OR IGNORE INTO recording_possible_matches(user_id, recording_id, candidate_recording_id, reason) VALUES (?, ?, ?, 'same_normalized_tags')")
                .bind(&user)
                .bind(recording_id.as_uuid().to_string())
                .bind(candidate)
                .execute(&mut *transaction)
                .await
                .map_err(import_error)?;
            true
        } else {
            false
        };
        transaction.commit().await.map_err(import_error)?;

        Ok(ImportOutcome {
            media_asset_id: projected_asset_id,
            track_id,
            recording_id,
            possible_match,
        })
    }
}

#[allow(clippy::too_many_arguments)]
async fn update_asset_and_metadata(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    user: &str,
    asset_id: MediaAssetId,
    input: &LocalImport,
    raw_tags_json: &str,
    normalized_title: &str,
    normalized_artists: &str,
    normalized_release: Option<&str>,
) -> AppResult<()> {
    sqlx::query("UPDATE media_assets SET normalized_path = ?, original_path = ?, content_fingerprint = ?, availability = 'available' WHERE user_id = ? AND media_asset_id = ?")
        .bind(&input.normalized_path)
        .bind(&input.original_path)
        .bind(&input.content_fingerprint)
        .bind(user)
        .bind(asset_id.as_uuid().to_string())
        .execute(&mut **transaction)
        .await
        .map_err(import_error)?;
    insert_metadata(
        transaction,
        user,
        asset_id,
        raw_tags_json,
        normalized_title,
        normalized_artists,
        normalized_release,
    )
    .await
}

async fn insert_metadata(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    user: &str,
    asset_id: MediaAssetId,
    raw_tags_json: &str,
    normalized_title: &str,
    normalized_artists: &str,
    normalized_release: Option<&str>,
) -> AppResult<()> {
    sqlx::query("INSERT INTO local_import_metadata(user_id, media_asset_id, raw_tags_json, normalized_title, normalized_artists_json, normalized_release, provenance) VALUES (?, ?, ?, ?, ?, ?, 'local_tags') ON CONFLICT(user_id, media_asset_id) DO UPDATE SET raw_tags_json = excluded.raw_tags_json, normalized_title = excluded.normalized_title, normalized_artists_json = excluded.normalized_artists_json, normalized_release = excluded.normalized_release, provenance = excluded.provenance")
        .bind(user)
        .bind(asset_id.as_uuid().to_string())
        .bind(raw_tags_json)
        .bind(normalized_title)
        .bind(normalized_artists)
        .bind(normalized_release)
        .execute(&mut **transaction)
        .await
        .map_err(import_error)?;
    Ok(())
}

async fn possible_match_exists(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    user: &str,
    recording_id: RecordingId,
) -> AppResult<bool> {
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM recording_possible_matches WHERE user_id = ? AND recording_id = ?",
    )
    .bind(user)
    .bind(recording_id.as_uuid().to_string())
    .fetch_one(&mut **transaction)
    .await
    .map_err(import_error)?;
    Ok(count > 0)
}

fn parse_media_asset_id(value: &str) -> AppResult<MediaAssetId> {
    let uuid = value.parse().map_err(import_error)?;
    MediaAssetId::try_from_uuid(uuid).map_err(import_error)
}

fn parse_track_id(value: &str) -> AppResult<TrackId> {
    let uuid = value.parse().map_err(import_error)?;
    TrackId::try_from_uuid(uuid).map_err(import_error)
}

fn parse_recording_id(value: &str) -> AppResult<RecordingId> {
    let uuid = value.parse().map_err(import_error)?;
    RecordingId::try_from_uuid(uuid).map_err(import_error)
}

fn import_error(error: impl std::fmt::Display) -> AppError {
    AppError {
        code: ErrorCode::StorageFailure,
        category: ErrorCategory::Storage,
        user_message: "library_import_failed".to_owned(),
        retryable: false,
        suggested_action: None,
        technical_context: Some(error.to_string()),
    }
}
