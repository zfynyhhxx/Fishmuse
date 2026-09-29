use async_trait::async_trait;
use fishmuse_domain::{
    AppError, AppResult, ErrorCategory, ErrorCode, LibraryItem, ListenSummary, PlayableSource,
    TrackId, TrackSummary, UserId,
};
use fishmuse_storage::{LibraryRepository, SqliteLibraryRepository};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SearchQuery {
    pub text: String,
    pub artist: Option<String>,
    pub release: Option<String>,
    pub limit: u32,
}

impl Default for SearchQuery {
    fn default() -> Self {
        Self {
            text: String::new(),
            artist: None,
            release: None,
            limit: 20,
        }
    }
}

#[async_trait]
pub trait LibraryQueryPort: Send + Sync {
    async fn search(&self, user_id: UserId, query: SearchQuery) -> AppResult<Vec<TrackSummary>>;

    async fn get_item(&self, user_id: UserId, id: TrackId) -> AppResult<Option<LibraryItem>>;

    async fn recent_listens(&self, user_id: UserId, limit: u32) -> AppResult<Vec<ListenSummary>>;

    async fn playable_source(
        &self,
        user_id: UserId,
        track_id: TrackId,
    ) -> AppResult<Option<PlayableSource>>;
}

#[async_trait]
impl LibraryQueryPort for SqliteLibraryRepository {
    async fn search(&self, user_id: UserId, query: SearchQuery) -> AppResult<Vec<TrackSummary>> {
        ensure_user(self, user_id)?;
        self.search_tracks(
            &query.text,
            query.artist.as_deref(),
            query.release.as_deref(),
            query.limit,
        )
        .await
    }

    async fn get_item(&self, user_id: UserId, id: TrackId) -> AppResult<Option<LibraryItem>> {
        ensure_user(self, user_id)?;
        LibraryRepository::find_track(self, id).await
    }

    async fn recent_listens(&self, user_id: UserId, limit: u32) -> AppResult<Vec<ListenSummary>> {
        ensure_user(self, user_id)?;
        SqliteLibraryRepository::recent_listens(self, limit).await
    }

    async fn playable_source(
        &self,
        user_id: UserId,
        track_id: TrackId,
    ) -> AppResult<Option<PlayableSource>> {
        ensure_user(self, user_id)?;
        SqliteLibraryRepository::playable_source(self, track_id).await
    }
}

fn ensure_user(repository: &SqliteLibraryRepository, user_id: UserId) -> AppResult<()> {
    if repository.user_id() == user_id {
        return Ok(());
    }
    Err(AppError {
        code: ErrorCode::Unauthorized,
        category: ErrorCategory::Library,
        user_message: "library_user_scope_mismatch".to_owned(),
        retryable: false,
        suggested_action: None,
        technical_context: None,
    })
}
