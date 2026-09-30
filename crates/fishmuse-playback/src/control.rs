use async_trait::async_trait;
use fishmuse_domain::AppResult;
use tokio::sync::broadcast;

use crate::{PlaybackCommand, PlaybackEvent, PlaybackManager, PlaybackSnapshot};

#[async_trait]
pub trait PlaybackControl: Send + Sync {
    async fn execute(&self, command: PlaybackCommand) -> AppResult<PlaybackSnapshot>;
    async fn snapshot(&self) -> AppResult<PlaybackSnapshot>;
    fn subscribe(&self) -> broadcast::Receiver<PlaybackEvent>;
}

#[async_trait]
impl PlaybackControl for PlaybackManager {
    async fn execute(&self, command: PlaybackCommand) -> AppResult<PlaybackSnapshot> {
        PlaybackManager::execute(self, command).await
    }

    async fn snapshot(&self) -> AppResult<PlaybackSnapshot> {
        PlaybackManager::snapshot(self).await
    }

    fn subscribe(&self) -> broadcast::Receiver<PlaybackEvent> {
        PlaybackManager::subscribe(self)
    }
}
