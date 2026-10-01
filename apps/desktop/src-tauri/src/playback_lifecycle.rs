use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use fishmuse_domain::{AppError, AppResult, ErrorCategory, ErrorCode};
use fishmuse_playback::{
    PlaybackCommand, PlaybackControl, PlaybackEvent, PlaybackServiceState, PlaybackServiceStatus,
    PlaybackSnapshot,
};
use tokio::sync::{Mutex, broadcast, watch};

const RECONNECT_PULSE_INTERVAL: Duration = Duration::from_millis(100);

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
            let mut reconnect_pulse = tokio::time::interval(RECONNECT_PULSE_INTERVAL);
            reconnect_pulse.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            reconnect_pulse.tick().await;
            loop {
                if *connection.borrow() == PlaybackConnectionState::Ready {
                    return Ok(());
                }
                tokio::select! {
                    changed = connection.changed() => {
                        changed.map_err(|_| {
                            startup_error(
                                ErrorCode::Unavailable,
                                "The playback service connection was interrupted.",
                                "connection state channel closed",
                            )
                        })?;
                    }
                    _ = reconnect_pulse.tick() => self.connection.reconnect_now(),
                }
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
            sync::atomic::{AtomicIsize, AtomicU32, Ordering},
        };
        use windows_sys::Win32::{
            Foundation::{CloseHandle, HWND, LPARAM, WAIT_TIMEOUT},
            System::Threading::{GetProcessId, WaitForSingleObject},
            UI::{
                Accessibility::{SetWinEventHook, UnhookWinEvent},
                Shell::{SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW},
                WindowsAndMessaging::{
                    DispatchMessageW, EVENT_SYSTEM_FOREGROUND, EnumWindows, GetForegroundWindow,
                    GetWindowThreadProcessId, MSG, MWMO_INPUTAVAILABLE,
                    MsgWaitForMultipleObjectsEx, PM_REMOVE, PeekMessageW, QS_ALLINPUT, SW_HIDE,
                    SetForegroundWindow, ShowWindow, TranslateMessage, WINEVENT_OUTOFCONTEXT,
                },
            },
        };

        static GUARDED_BACKEND_PROCESS_ID: AtomicU32 = AtomicU32::new(0);
        static FISHMUSE_WINDOW: AtomicIsize = AtomicIsize::new(0);
        static FISHMUSE_PROCESS_ID: AtomicU32 = AtomicU32::new(0);

        #[derive(Clone, Copy)]
        struct BackendWindowGuard {
            process_id: u32,
            fishmuse_window: isize,
            fishmuse_process_id: u32,
        }

        unsafe fn hide_backend_window(window: HWND, context: BackendWindowGuard) {
            let mut process_id = 0;
            unsafe {
                GetWindowThreadProcessId(window, &mut process_id);
            }
            if process_id != context.process_id {
                return;
            }

            let was_foreground = !window.is_null() && window == unsafe { GetForegroundWindow() };
            unsafe {
                ShowWindow(window, SW_HIDE);
            }
            if was_foreground && context.fishmuse_window != 0 {
                let fishmuse_window = context.fishmuse_window as HWND;
                let mut fishmuse_process_id = 0;
                unsafe {
                    GetWindowThreadProcessId(fishmuse_window, &mut fishmuse_process_id);
                }
                if fishmuse_process_id == context.fishmuse_process_id {
                    unsafe {
                        SetForegroundWindow(fishmuse_window);
                    }
                }
            }
        }

        unsafe extern "system" fn keep_backend_hidden(
            window: HWND,
            context: LPARAM,
        ) -> windows_sys::core::BOOL {
            let context = unsafe { *(context as *const BackendWindowGuard) };
            unsafe { hide_backend_window(window, context) };
            1
        }

        unsafe extern "system" fn keep_backend_from_taking_foreground(
            _hook: windows_sys::Win32::UI::Accessibility::HWINEVENTHOOK,
            event: u32,
            window: HWND,
            _object_id: i32,
            _child_id: i32,
            _event_thread: u32,
            _event_time: u32,
        ) {
            if event != EVENT_SYSTEM_FOREGROUND || window.is_null() {
                return;
            }
            let context = BackendWindowGuard {
                process_id: GUARDED_BACKEND_PROCESS_ID.load(Ordering::Acquire),
                fishmuse_window: FISHMUSE_WINDOW.load(Ordering::Acquire),
                fishmuse_process_id: FISHMUSE_PROCESS_ID.load(Ordering::Acquire),
            };
            unsafe { hide_backend_window(window, context) };
        }

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

        let previous_foreground = unsafe { GetForegroundWindow() };
        let mut previous_foreground_process_id = 0;
        if !previous_foreground.is_null() {
            unsafe {
                GetWindowThreadProcessId(previous_foreground, &mut previous_foreground_process_id);
            }
        }
        let fishmuse_foreground = (previous_foreground_process_id == std::process::id())
            .then_some(previous_foreground as isize);
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
            #[cfg(feature = "live-e2e")]
            if process_id != 0 {
                record_live_backend_process(process_id, info.hProcess);
            }
            if process_id != 0 {
                let fishmuse_window = fishmuse_foreground.unwrap_or_default();
                let process_handle = info.hProcess as isize;
                std::thread::spawn(move || {
                    let process_handle = process_handle as _;
                    let context = BackendWindowGuard {
                        process_id,
                        fishmuse_window,
                        fishmuse_process_id: std::process::id(),
                    };
                    GUARDED_BACKEND_PROCESS_ID.store(process_id, Ordering::Release);
                    FISHMUSE_WINDOW.store(fishmuse_window, Ordering::Release);
                    FISHMUSE_PROCESS_ID.store(context.fishmuse_process_id, Ordering::Release);
                    let foreground_hook = unsafe {
                        SetWinEventHook(
                            EVENT_SYSTEM_FOREGROUND,
                            EVENT_SYSTEM_FOREGROUND,
                            null_mut(),
                            Some(keep_backend_from_taking_foreground),
                            process_id,
                            0,
                            WINEVENT_OUTOFCONTEXT,
                        )
                    };
                    let deadline = std::time::Instant::now() + Duration::from_secs(5);
                    while std::time::Instant::now() < deadline
                        && unsafe { WaitForSingleObject(process_handle, 0) } == WAIT_TIMEOUT
                    {
                        unsafe {
                            EnumWindows(Some(keep_backend_hidden), (&raw const context) as LPARAM);
                            let mut message: MSG = zeroed();
                            while PeekMessageW(&raw mut message, null_mut(), 0, 0, PM_REMOVE) != 0 {
                                TranslateMessage(&raw const message);
                                DispatchMessageW(&raw const message);
                            }
                            MsgWaitForMultipleObjectsEx(
                                1,
                                &raw const process_handle,
                                10,
                                QS_ALLINPUT,
                                MWMO_INPUTAVAILABLE,
                            );
                        }
                    }
                    if !foreground_hook.is_null() {
                        unsafe {
                            UnhookWinEvent(foreground_hook);
                        }
                    }
                    if GUARDED_BACKEND_PROCESS_ID
                        .compare_exchange(process_id, 0, Ordering::AcqRel, Ordering::Acquire)
                        .is_ok()
                    {
                        FISHMUSE_WINDOW.store(0, Ordering::Release);
                        FISHMUSE_PROCESS_ID.store(0, Ordering::Release);
                    }
                    unsafe {
                        CloseHandle(process_handle);
                    }
                });
            } else {
                unsafe {
                    CloseHandle(info.hProcess);
                }
            }
            (process_id != 0).then_some(process_id)
        };
        Ok(LaunchOutcome { process_id })
    }
}

#[cfg(all(windows, feature = "live-e2e"))]
fn record_live_backend_process(
    process_id: u32,
    process_handle: windows_sys::Win32::Foundation::HANDLE,
) {
    let Some(path) = std::env::var_os("FISHMUSE_LIVE_BACKEND_PID") else {
        return;
    };
    let _ = record_live_process_identity(path, process_id, process_handle);
}

#[cfg(all(windows, feature = "live-e2e"))]
pub(crate) fn record_live_process_identity(
    path: impl AsRef<std::path::Path>,
    process_id: u32,
    process_handle: windows_sys::Win32::Foundation::HANDLE,
) -> std::io::Result<()> {
    use windows_sys::Win32::{Foundation::FILETIME, System::Threading::GetProcessTimes};

    let mut creation = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let mut exit = creation;
    let mut kernel = creation;
    let mut user = creation;
    if unsafe {
        GetProcessTimes(
            process_handle,
            &raw mut creation,
            &raw mut exit,
            &raw mut kernel,
            &raw mut user,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error());
    }
    let creation_time =
        (u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime);
    std::fs::write(path, format!("{process_id}|{creation_time}"))
}
