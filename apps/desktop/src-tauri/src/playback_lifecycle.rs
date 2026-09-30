use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use fishmuse_domain::{AppError, AppResult, ErrorCategory, ErrorCode};
use fishmuse_playback::{
    PlaybackCommand, PlaybackControl, PlaybackEvent, PlaybackServiceState, PlaybackServiceStatus,
    PlaybackSnapshot,
};
use tokio::sync::{Mutex, broadcast, watch};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlaybackConnectionState {
    Ready,
    Unavailable,
}

#[async_trait]
pub trait PlaybackConnection: Send + Sync {
    fn subscribe(&self) -> watch::Receiver<PlaybackConnectionState>;
    fn reconnect_now(&self);
    async fn shutdown(&self);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LaunchOutcome {
    pub process_id: Option<u32>,
}

pub trait PlaybackBackendLauncher: Send + Sync {
    fn launch_hidden(&self) -> AppResult<LaunchOutcome>;
}

pub struct ManagedPlaybackService {
    control: Arc<dyn PlaybackControl>,
    launcher: Arc<dyn PlaybackBackendLauncher>,
    connection: Arc<dyn PlaybackConnection>,
    readiness_timeout: Duration,
    startup: Mutex<()>,
    service_state: watch::Sender<PlaybackServiceState>,
}

impl ManagedPlaybackService {
    #[must_use]
    pub fn new<C, L, R>(
        control: Arc<C>,
        launcher: Arc<L>,
        connection: Arc<R>,
        readiness_timeout: Duration,
    ) -> Self
    where
        C: PlaybackControl + 'static,
        L: PlaybackBackendLauncher + 'static,
        R: PlaybackConnection + 'static,
    {
        let initial_status = if *connection.subscribe().borrow() == PlaybackConnectionState::Ready {
            PlaybackServiceStatus::Ready
        } else {
            PlaybackServiceStatus::Disconnected
        };
        let (service_state, _) = watch::channel(PlaybackServiceState {
            status: initial_status,
            implementation: None,
        });
        Self {
            control,
            launcher,
            connection,
            readiness_timeout,
            startup: Mutex::new(()),
            service_state,
        }
    }

    #[must_use]
    pub fn service_state(&self) -> PlaybackServiceState {
        self.service_state.borrow().clone()
    }

    #[must_use]
    pub fn subscribe_service_state(&self) -> watch::Receiver<PlaybackServiceState> {
        self.service_state.subscribe()
    }

    pub async fn retry(&self) -> AppResult<()> {
        self.ensure_ready().await
    }

    pub async fn shutdown(&self) {
        self.connection.shutdown().await;
    }

    async fn ensure_ready(&self) -> AppResult<()> {
        if *self.connection.subscribe().borrow() == PlaybackConnectionState::Ready {
            self.publish_status(PlaybackServiceStatus::Ready);
            return Ok(());
        }

        let _startup = self.startup.lock().await;
        let mut connection = self.connection.subscribe();
        if *connection.borrow() == PlaybackConnectionState::Ready {
            self.publish_status(PlaybackServiceStatus::Ready);
            return Ok(());
        }

        self.publish_status(PlaybackServiceStatus::Starting);
        if let Err(error) = self.launcher.launch_hidden() {
            self.publish_status(PlaybackServiceStatus::Unavailable);
            return Err(startup_error(
                error.code,
                "The playback service could not be started.",
                error.user_message,
            ));
        }
        self.connection.reconnect_now();

        let wait_until_ready = async {
            loop {
                if *connection.borrow() == PlaybackConnectionState::Ready {
                    return Ok(());
                }
                connection.changed().await.map_err(|_| {
                    startup_error(
                        ErrorCode::Unavailable,
                        "The playback service connection was interrupted.",
                        "connection state channel closed",
                    )
                })?;
            }
        };
        match tokio::time::timeout(self.readiness_timeout, wait_until_ready).await {
            Ok(Ok(())) => {
                self.publish_status(PlaybackServiceStatus::Ready);
                Ok(())
            }
            Ok(Err(error)) => {
                self.publish_status(PlaybackServiceStatus::Unavailable);
                Err(error)
            }
            Err(_) => {
                self.publish_status(PlaybackServiceStatus::Unavailable);
                Err(startup_error(
                    ErrorCode::Unavailable,
                    "The playback service did not become ready in time.",
                    "managed playback readiness timeout",
                ))
            }
        }
    }

    fn publish_status(&self, status: PlaybackServiceStatus) {
        if self.service_state.borrow().status != status {
            self.service_state
                .send_modify(|state| state.status = status);
        }
    }
}

#[async_trait]
impl PlaybackControl for ManagedPlaybackService {
    async fn execute(&self, command: PlaybackCommand) -> AppResult<PlaybackSnapshot> {
        self.ensure_ready().await?;
        self.control.execute(command).await
    }

    async fn snapshot(&self) -> AppResult<PlaybackSnapshot> {
        self.control.snapshot().await
    }

    fn subscribe(&self) -> broadcast::Receiver<PlaybackEvent> {
        self.control.subscribe()
    }
}

fn startup_error(
    code: ErrorCode,
    user_message: &str,
    technical_context: impl Into<String>,
) -> AppError {
    AppError {
        code,
        category: ErrorCategory::Playback,
        user_message: user_message.to_owned(),
        retryable: true,
        suggested_action: Some("open_advanced_playback_diagnostics".to_owned()),
        technical_context: Some(technical_context.into()),
    }
}

pub struct UnavailablePlaybackLauncher;

impl PlaybackBackendLauncher for UnavailablePlaybackLauncher {
    fn launch_hidden(&self) -> AppResult<LaunchOutcome> {
        Err(startup_error(
            ErrorCode::BackendUnavailable,
            "The playback service could not be started.",
            "no platform playback launcher is available",
        ))
    }
}

#[cfg(all(windows, not(feature = "e2e")))]
pub struct WindowsPlaybackLauncher;

#[cfg(all(windows, not(feature = "e2e")))]
impl PlaybackBackendLauncher for WindowsPlaybackLauncher {
    fn launch_hidden(&self) -> AppResult<LaunchOutcome> {
        use std::{
            ffi::OsStr,
            mem::{size_of, zeroed},
            os::windows::ffi::OsStrExt,
            ptr::null_mut,
        };
        use windows_sys::Win32::{
            Foundation::CloseHandle,
            System::Threading::GetProcessId,
            UI::{
                Shell::{SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW},
                WindowsAndMessaging::SW_HIDE,
            },
        };

        let verb = OsStr::new("open")
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let executable = OsStr::new("foobar2000.exe")
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let mut info: SHELLEXECUTEINFOW = unsafe { zeroed() };
        info.cbSize =
            u32::try_from(size_of::<SHELLEXECUTEINFOW>()).expect("SHELLEXECUTEINFOW size fits u32");
        info.fMask = SEE_MASK_NOCLOSEPROCESS;
        info.hwnd = null_mut();
        info.lpVerb = verb.as_ptr();
        info.lpFile = executable.as_ptr();
        info.nShow = SW_HIDE;

        if unsafe { ShellExecuteExW(&mut info) } == 0 {
            return Err(startup_error(
                ErrorCode::BackendUnavailable,
                "The playback service could not be started.",
                "ShellExecuteExW failed",
            ));
        }
        let process_id = if info.hProcess.is_null() {
            None
        } else {
            let process_id = unsafe { GetProcessId(info.hProcess) };
            unsafe {
                CloseHandle(info.hProcess);
            }
            (process_id != 0).then_some(process_id)
        };
        Ok(LaunchOutcome { process_id })
    }
}
