mod library;
mod playback;
mod registry;

pub use registry::{ToolExecutor, ToolRegistry};

use std::sync::Arc;

use async_trait::async_trait;
use fishmuse_domain::{AppResult, UserId};
use fishmuse_library::LibraryQueryPort;
use fishmuse_playback::PlaybackControl;
use serde_json::Value;

pub struct MusicToolExecutor {
    user_id: UserId,
    library: Arc<dyn LibraryQueryPort>,
    playback: Arc<dyn PlaybackControl>,
}

impl MusicToolExecutor {
    #[must_use]
    pub fn new<L>(user_id: UserId, library: Arc<L>, playback: Arc<dyn PlaybackControl>) -> Self
    where
        L: LibraryQueryPort + 'static,
    {
        Self {
            user_id,
            library,
            playback,
        }
    }
}

#[async_trait]
impl ToolExecutor for MusicToolExecutor {
    async fn execute(&self, name: &str, arguments: &Value) -> AppResult<Value> {
        match name {
            "search_library" | "get_library_item" | "get_recent_listens" => {
                library::execute(self.library.as_ref(), self.user_id, name, arguments).await
            }
            _ => {
                playback::execute(
                    self.library.as_ref(),
                    self.playback.as_ref(),
                    self.user_id,
                    name,
                    arguments,
                )
                .await
            }
        }
    }
}
