mod backend;
mod presentation;
mod protocol;
#[cfg(windows)]
mod windows_native;

use backend::{request_backend_shutdown, AppRuntime, UsageState};
use serde::{Deserialize, Deserializer, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};

const NORMAL_WINDOW_SIZE: (f64, f64) = (360.0, 220.0);
const NORMAL_WINDOW_MIN_SIZE: (f64, f64) = (320.0, 220.0);
const ULTRA_COMPACT_WINDOW_SIZE: (f64, f64) = (300.0, 156.0);
const ULTRA_COMPACT_WINDOW_MIN_SIZE: (f64, f64) = ULTRA_COMPACT_WINDOW_SIZE;
const MAX_WINDOW_DIMENSION: f64 = 4096.0;
const RESIZE_SAVE_DEBOUNCE: Duration = Duration::from_millis(300);
const RESIZE_EVENT_SUPPRESSION: Duration = Duration::from_millis(700);

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowSize {
    pub width: f64,
    pub height: f64,
}

impl WindowSize {
    fn is_valid_for(self, ultra_compact: bool) -> bool {
        let (min_width, _) = window_minimum(ultra_compact);
        let fixed_height = if ultra_compact {
            ULTRA_COMPACT_WINDOW_SIZE.1
        } else {
            NORMAL_WINDOW_SIZE.1
        };
        let height_is_valid = (self.height - fixed_height).abs() <= 0.5;
        self.width.is_finite()
            && self.height.is_finite()
            && self.width >= min_width
            && height_is_valid
            && self.width <= MAX_WINDOW_DIMENSION
            && self.height <= MAX_WINDOW_DIMENSION
    }

    fn from_physical(width: u32, height: u32, scale_factor: f64) -> Option<Self> {
        if !scale_factor.is_finite() || scale_factor <= 0.0 {
            return None;
        }
        Some(Self {
            width: f64::from(width) / scale_factor,
            height: f64::from(height) / scale_factor,
        })
    }
}

fn default_normal_window_size() -> WindowSize {
    WindowSize {
        width: NORMAL_WINDOW_SIZE.0,
        height: NORMAL_WINDOW_SIZE.1,
    }
}

fn default_ultra_compact_window_size() -> WindowSize {
    WindowSize {
        width: ULTRA_COMPACT_WINDOW_SIZE.0,
        height: ULTRA_COMPACT_WINDOW_SIZE.1,
    }
}

fn window_minimum(ultra_compact: bool) -> (f64, f64) {
    if ultra_compact {
        ULTRA_COMPACT_WINDOW_MIN_SIZE
    } else {
        NORMAL_WINDOW_MIN_SIZE
    }
}

#[cfg(test)]
fn window_maximum(ultra_compact: bool) -> Option<WindowSize> {
    Some(WindowSize {
        width: MAX_WINDOW_DIMENSION,
        height: if ultra_compact {
            ULTRA_COMPACT_WINDOW_SIZE.1
        } else {
            NORMAL_WINDOW_SIZE.1
        },
    })
}

fn set_window_size_for_mode(
    window: &tauri::WebviewWindow,
    ultra_compact: bool,
    size: WindowSize,
) -> tauri::Result<()> {
    let minimum = (window_minimum(ultra_compact).0, size.height);
    #[cfg(windows)]
    windows_native::set_target(window, size.height, minimum.0);
    window.set_max_size(None::<tauri::LogicalSize<f64>>)?;
    window.set_min_size(None::<tauri::LogicalSize<f64>>)?;
    window.set_min_size(Some(tauri::LogicalSize::new(minimum.0, minimum.1)))?;
    window.set_max_size(Some(tauri::LogicalSize::new(
        MAX_WINDOW_DIMENSION,
        size.height,
    )))?;
    window.set_size(tauri::LogicalSize::new(size.width, size.height))
}

fn window_size_for(settings: &Settings, ultra_compact: bool) -> WindowSize {
    if ultra_compact {
        settings.ultra_compact_window_size
    } else {
        settings.normal_window_size
    }
}

fn set_window_size_for(settings: &mut Settings, ultra_compact: bool, size: WindowSize) {
    if ultra_compact {
        settings.ultra_compact_window_size = size;
    } else {
        settings.normal_window_size = size;
    }
}

fn normalized_window_size(size: WindowSize, ultra_compact: bool) -> WindowSize {
    let default = if ultra_compact {
        default_ultra_compact_window_size()
    } else {
        default_normal_window_size()
    };
    let minimum = window_minimum(ultra_compact);
    let width = if size.width.is_finite()
        && size.width >= minimum.0
        && size.width <= MAX_WINDOW_DIMENSION
    {
        size.width
    } else {
        default.width
    };
    WindowSize {
        width,
        height: default.height,
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CloseAction {
    Ask,
    Minimize,
    Quit,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    #[default]
    En,
    Es,
}

fn deserialize_language<'de, D>(deserializer: D) -> Result<Language, D::Error>
where
    D: Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(if value.as_str() == Some("es") {
        Language::Es
    } else {
        Language::En
    })
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Settings {
    #[serde(rename = "threshold5h")]
    pub threshold_5h: f64,
    #[serde(rename = "thresholdWeekly")]
    pub threshold_weekly: f64,
    #[serde(default, deserialize_with = "deserialize_language")]
    pub language: Language,
    // Kept so settings files from older versions remain readable.
    #[serde(default = "default_ui_scale")]
    pub ui_scale: u16,
    #[serde(default = "default_always_on_top")]
    pub always_on_top: bool,
    #[serde(default = "default_close_action")]
    pub close_action: CloseAction,
    #[serde(default)]
    pub ultra_compact: bool,
    #[serde(default = "default_normal_window_size")]
    pub normal_window_size: WindowSize,
    #[serde(default = "default_ultra_compact_window_size")]
    pub ultra_compact_window_size: WindowSize,
}

struct LanguageState(Mutex<Language>);

#[derive(Clone)]
struct SettingsFileLock(Arc<Mutex<()>>);

#[derive(Clone)]
struct WindowModeTracker(Arc<Mutex<WindowModeSnapshot>>);

struct PanelReadyState(AtomicBool);

#[derive(Clone)]
struct WindowSizeSaveChannel(mpsc::Sender<WindowSizeSaveRequest>);

#[derive(Clone, Copy)]
struct WindowModeSnapshot {
    ultra_compact: bool,
    revision: u64,
    suppress_until: Instant,
    settings_height: Option<f64>,
}

#[derive(Clone, Copy)]
struct WindowSizeSaveRequest {
    ultra_compact: bool,
    revision: u64,
    size: WindowSize,
}

fn default_ui_scale() -> u16 {
    100
}

fn default_always_on_top() -> bool {
    true
}

fn default_close_action() -> CloseAction {
    CloseAction::Ask
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            threshold_5h: 20.0,
            threshold_weekly: 20.0,
            language: Language::En,
            ui_scale: default_ui_scale(),
            always_on_top: default_always_on_top(),
            close_action: default_close_action(),
            ultra_compact: false,
            normal_window_size: default_normal_window_size(),
            ultra_compact_window_size: default_ultra_compact_window_size(),
        }
    }
}

impl Settings {
    fn is_valid(&self) -> bool {
        self.threshold_5h.is_finite()
            && self.threshold_weekly.is_finite()
            && (0.0..=100.0).contains(&self.threshold_5h)
            && (0.0..=100.0).contains(&self.threshold_weekly)
            && self.threshold_5h.fract() == 0.0
            && self.threshold_weekly.fract() == 0.0
            && (80..=150).contains(&self.ui_scale)
            && self.ui_scale % 10 == 0
    }
}

#[tauri::command]
fn get_usage_state(runtime: State<'_, AppRuntime>) -> UsageState {
    runtime
        .state
        .lock()
        .map(|snapshot| snapshot.clone())
        .unwrap_or_default()
}

#[tauri::command]
fn load_settings(app: AppHandle) -> Result<Settings, String> {
    if let Some(file_lock) = app.try_state::<SettingsFileLock>() {
        if let Ok(_guard) = file_lock.0.lock() {
            return try_read_settings_file(&app);
        }
    }
    try_read_settings_file(&app)
}

#[tauri::command]
fn save_settings(app: AppHandle, settings: serde_json::Value) -> Result<Settings, String> {
    let file_lock = app.try_state::<SettingsFileLock>();
    let _guard = match file_lock.as_ref() {
        Some(lock) => Some(
            lock.0
                .lock()
                .map_err(|_| "No se pudo guardar la configuración.".to_owned())?,
        ),
        None => None,
    };
    let current = try_read_settings_file(&app)?;
    let mut merged =
        serde_json::to_value(current).map_err(|_| "Could not save preferences".to_owned())?;
    let patch = settings.as_object().ok_or("Invalid preferences")?;
    for (key, value) in patch {
        if ![
            "threshold5h",
            "thresholdWeekly",
            "language",
            "uiScale",
            "alwaysOnTop",
            "closeAction",
            "ultraCompact",
        ]
        .contains(&key.as_str())
        {
            return Err("Invalid preference field".into());
        }
        merged[key] = value.clone();
    }
    let settings: Settings =
        serde_json::from_value(merged).map_err(|_| "Invalid preferences".to_owned())?;
    if !settings.is_valid() {
        return Err("Invalid preferences".into());
    }
    write_settings_file(&app, &settings)?;
    let language_changed = app
        .try_state::<LanguageState>()
        .and_then(|state| {
            state.0.lock().ok().map(|mut current| {
                let changed = *current != settings.language;
                *current = settings.language;
                changed
            })
        })
        .unwrap_or(false);
    if language_changed {
        update_tray_language(&app, settings.language);
    }
    refresh_tray_indicators(&app);
    let _ = app.emit("preferences-updated", settings.clone());
    Ok(settings)
}

fn read_settings_file(app: &AppHandle) -> Settings {
    try_read_settings_file(app).unwrap_or_default()
}

fn decode_settings(value: serde_json::Value) -> Result<Settings, String> {
    let source = value.as_object().ok_or("Invalid preferences file")?;
    let mut normalized =
        serde_json::to_value(Settings::default()).map_err(|_| "Invalid defaults")?;
    for (key, value) in source {
        let valid = match key.as_str() {
            "threshold5h" | "thresholdWeekly" => value
                .as_f64()
                .is_some_and(|n| n.is_finite() && (0.0..=100.0).contains(&n)),
            "uiScale" => value
                .as_u64()
                .is_some_and(|n| (80..=150).contains(&n) && n % 10 == 0),
            "language" => matches!(value.as_str(), Some("en" | "es")),
            "closeAction" => matches!(value.as_str(), Some("ask" | "minimize" | "quit")),
            "alwaysOnTop" | "ultraCompact" => value.is_boolean(),
            "normalWindowSize" | "ultraCompactWindowSize" => {
                serde_json::from_value::<WindowSize>(value.clone()).is_ok()
            }
            _ => false,
        };
        if valid {
            normalized[key] = if key == "threshold5h" || key == "thresholdWeekly" {
                serde_json::json!(value.as_f64().unwrap_or(20.0).round())
            } else {
                value.clone()
            };
        }
    }
    let mut settings: Settings =
        serde_json::from_value(normalized).map_err(|_| "Invalid preferences file")?;
    settings.normal_window_size = normalized_window_size(settings.normal_window_size, false);
    settings.ultra_compact_window_size =
        normalized_window_size(settings.ultra_compact_window_size, true);
    Ok(settings)
}

fn try_read_settings_file(app: &AppHandle) -> Result<Settings, String> {
    let path = settings_path(app).map_err(|_| "Could not load preferences".to_owned())?;
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Settings::default())
        }
        Err(_) => return Err("Could not load preferences".into()),
    };
    decode_settings(
        serde_json::from_str(&contents).map_err(|_| "Could not load preferences".to_owned())?,
    )
}

fn write_settings_file(app: &AppHandle, settings: &Settings) -> Result<(), String> {
    let path = settings_path(app).map_err(|_| "Could not save preferences".to_owned())?;
    write_settings_path(&path, settings).map_err(|_| "Could not save preferences".to_owned())
}

fn write_settings_path(path: &std::path::Path, settings: &Settings) -> std::io::Result<()> {
    use std::io::Write;
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("Invalid settings path"))?;
    fs::create_dir_all(parent)?;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temporary = parent.join(format!("settings-{}-{nonce}.tmp", std::process::id()));
    let result = (|| {
        let serialized = serde_json::to_vec_pretty(settings)?;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(&serialized)?;
        file.sync_all()?;
        drop(file);
        #[cfg(windows)]
        windows_native::replace_file(&temporary, path)?;
        #[cfg(not(windows))]
        fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

#[tauri::command]
fn get_app_version() -> String {
    env!("CARGO_PKG_VERSION").to_owned()
}

fn run_window_mode_transition<T>(
    tracker: &WindowModeTracker,
    ultra_compact: bool,
    resize_window: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let previous_mode = {
        let mut mode = tracker
            .0
            .lock()
            .map_err(|_| "No se pudo cambiar el tamaño de la ventana.".to_owned())?;
        let previous_mode = (mode.ultra_compact, mode.settings_height);
        mode.ultra_compact = ultra_compact;
        mode.settings_height = None;
        mode.revision = mode.revision.wrapping_add(1);
        mode.suppress_until = Instant::now() + RESIZE_EVENT_SUPPRESSION;
        previous_mode
    };

    match resize_window() {
        Ok(result) => Ok(result),
        Err(error) => {
            if let Ok(mut mode) = tracker.0.lock() {
                mode.ultra_compact = previous_mode.0;
                mode.settings_height = previous_mode.1;
                mode.revision = mode.revision.wrapping_add(1);
                mode.suppress_until = Instant::now() + RESIZE_EVENT_SUPPRESSION;
            }
            Err(error)
        }
    }
}

#[cfg(test)]
fn window_mode_sizes(ultra_compact: bool) -> ((f64, f64), (f64, f64)) {
    if ultra_compact {
        (ULTRA_COMPACT_WINDOW_MIN_SIZE, ULTRA_COMPACT_WINDOW_SIZE)
    } else {
        (NORMAL_WINDOW_MIN_SIZE, NORMAL_WINDOW_SIZE)
    }
}

#[tauri::command]
fn set_ultra_compact(
    app: AppHandle,
    ultra_compact: bool,
    preferred_ultra_compact: Option<bool>,
) -> Result<WindowSize, String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "No se pudo cambiar el tamaño de la ventana.".to_owned())?;
    let tracker = app
        .try_state::<WindowModeTracker>()
        .ok_or_else(|| "No se pudo cambiar el tamaño de la ventana.".to_owned())?;
    let scale_factor = window
        .scale_factor()
        .map_err(|_| "No se pudo cambiar el tamaño de la ventana.".to_owned())?;
    let current_size = window
        .inner_size()
        .ok()
        .and_then(|size| WindowSize::from_physical(size.width, size.height, scale_factor));
    let (size, previous_preference) = {
        let file_lock = app.try_state::<SettingsFileLock>();
        let _guard = match file_lock.as_ref() {
            Some(lock) => Some(
                lock.0
                    .lock()
                    .map_err(|_| "No se pudo cambiar el tamaño de la ventana.".to_owned())?,
            ),
            None => None,
        };
        let mode = tracker
            .0
            .lock()
            .map_err(|_| "No se pudo cambiar el tamaño de la ventana.".to_owned())?;
        let mut settings = try_read_settings_file(&app)?;
        let previous_preference = preferred_ultra_compact.map(|_| settings.ultra_compact);
        if let Some(preferred) = preferred_ultra_compact {
            settings.ultra_compact = preferred;
        }
        let valid_current_size = current_size
            .filter(|size| mode.settings_height.is_none() && size.is_valid_for(mode.ultra_compact));
        if let Some(size) = valid_current_size {
            set_window_size_for(&mut settings, mode.ultra_compact, size);
        }
        if previous_preference.is_some() || valid_current_size.is_some() {
            write_settings_file(&app, &settings)?;
        }
        (
            normalized_window_size(window_size_for(&settings, ultra_compact), ultra_compact),
            previous_preference,
        )
    };
    if let Err(error) = run_window_mode_transition(&tracker, ultra_compact, || {
        set_window_size_for_mode(&window, ultra_compact, size)
            .map_err(|_| "No se pudo cambiar el tamaño de la ventana.".to_owned())
    }) {
        if let Some(previous) = previous_preference {
            if let Some(file_lock) = app.try_state::<SettingsFileLock>() {
                if let Ok(_guard) = file_lock.0.lock() {
                    let mut settings = read_settings_file(&app);
                    settings.ultra_compact = previous;
                    let _ = write_settings_file(&app, &settings);
                }
            }
        }
        return Err(error);
    }

    if let Some(writer) = app
        .try_state::<WindowSizeSaveChannel>()
        .map(|state| state.0.clone())
    {
        schedule_window_size_checkpoint(app.clone(), window, tracker.inner().clone(), writer);
    }
    Ok(size)
}

#[tauri::command]
fn fit_settings_window(app: AppHandle, content_height: f64) -> Result<WindowSize, String> {
    if !content_height.is_finite() || !(120.0..=1024.0).contains(&content_height) {
        return Err("Invalid settings height".into());
    }
    let window = app.get_webview_window("main").ok_or("Window unavailable")?;
    let scale = window.scale_factor().map_err(|_| "Window unavailable")?;
    let current = window.inner_size().map_err(|_| "Window unavailable")?;
    let size = WindowSize {
        width: (f64::from(current.width) / scale).max(NORMAL_WINDOW_MIN_SIZE.0),
        height: content_height.ceil(),
    };
    let tracker = app.state::<WindowModeTracker>();
    let previous = {
        let mut mode = tracker.0.lock().map_err(|_| "Window unavailable")?;
        let previous = *mode;
        mode.settings_height = Some(size.height);
        mode.revision = mode.revision.wrapping_add(1);
        mode.suppress_until = Instant::now() + RESIZE_EVENT_SUPPRESSION;
        previous
    };
    if set_window_size_for_mode(&window, false, size).is_err() {
        if let Ok(mut mode) = tracker.0.lock() {
            *mode = previous;
        }
        let original = WindowSize {
            width: f64::from(current.width) / scale,
            height: f64::from(current.height) / scale,
        };
        let _ = set_window_size_for_mode(&window, previous.ultra_compact, original);
        return Err("Could not resize settings".into());
    }
    Ok(size)
}

#[tauri::command]
fn get_native_window_mode(tracker: State<'_, WindowModeTracker>) -> Result<bool, String> {
    tracker
        .0
        .lock()
        .map(|mode| mode.ultra_compact)
        .map_err(|_| "No se pudo leer el tamaño de la ventana.".to_owned())
}

#[tauri::command]
fn show_main_panel(app: AppHandle) -> Result<(), String> {
    if let Some(ready) = app.try_state::<PanelReadyState>() {
        ready.0.store(true, Ordering::Release);
    }
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "No se pudo mostrar la ventana.".to_owned())?;
    window
        .show()
        .map_err(|_| "No se pudo mostrar la ventana.".to_owned())
}

#[tauri::command]
fn minimize_panel(app: AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.minimize();
    }
}

#[tauri::command]
fn set_always_on_top(app: AppHandle, always_on_top: bool) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "No se pudo cambiar el comportamiento de la ventana.".to_owned())?;
    window
        .set_always_on_top(always_on_top)
        .map_err(|_| "No se pudo cambiar el comportamiento de la ventana.".to_owned())
}

#[tauri::command]
fn quit_app(app: AppHandle, runtime: State<'_, AppRuntime>) {
    request_backend_shutdown(&runtime.commands);
    app.exit(0);
}

fn settings_path(app: &AppHandle) -> tauri::Result<PathBuf> {
    Ok(app.path().app_config_dir()?.join("settings.json"))
}

fn show_panel(app: &AppHandle) {
    if app
        .try_state::<PanelReadyState>()
        .is_some_and(|ready| !ready.0.load(Ordering::Acquire))
    {
        return;
    }
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        let _ = window.emit("panel-restored", ());
    }
}

fn queue_window_size_save(
    writer: &mpsc::Sender<WindowSizeSaveRequest>,
    tracker: &WindowModeTracker,
    size: WindowSize,
) {
    if let Ok(snapshot) = tracker.0.lock() {
        if snapshot.settings_height.is_some()
            || Instant::now() < snapshot.suppress_until
            || !size.is_valid_for(snapshot.ultra_compact)
        {
            return;
        }
        let _ = writer.send(WindowSizeSaveRequest {
            ultra_compact: snapshot.ultra_compact,
            revision: snapshot.revision,
            size,
        });
    }
}

fn queue_current_window_size_save(
    window: &tauri::WebviewWindow,
    writer: &mpsc::Sender<WindowSizeSaveRequest>,
    tracker: &WindowModeTracker,
) {
    let scale_factor = window.scale_factor().unwrap_or(1.0);
    let Some(size) = window
        .inner_size()
        .ok()
        .and_then(|size| WindowSize::from_physical(size.width, size.height, scale_factor))
    else {
        return;
    };
    queue_window_size_save(writer, tracker, size);
}

fn schedule_window_size_checkpoint(
    app: AppHandle,
    window: tauri::WebviewWindow,
    tracker: WindowModeTracker,
    writer: mpsc::Sender<WindowSizeSaveRequest>,
) {
    let _ = thread::Builder::new()
        .name("window-size-checkpoint".into())
        .spawn(move || {
            thread::sleep(RESIZE_EVENT_SUPPRESSION);
            let _ = app.run_on_main_thread(move || {
                queue_current_window_size_save(&window, &writer, &tracker);
            });
        });
}

fn persist_window_size_request(
    app: &AppHandle,
    tracker: &WindowModeTracker,
    request: WindowSizeSaveRequest,
) {
    let Some(file_lock) = app.try_state::<SettingsFileLock>() else {
        return;
    };
    let Ok(_guard) = file_lock.0.lock() else {
        return;
    };
    let Ok(snapshot) = tracker.0.lock() else {
        return;
    };
    if snapshot.settings_height.is_some()
        || snapshot.revision != request.revision
        || snapshot.ultra_compact != request.ultra_compact
        || !request.size.is_valid_for(request.ultra_compact)
    {
        return;
    }
    let Ok(mut settings) = try_read_settings_file(app) else {
        return;
    };
    set_window_size_for(&mut settings, request.ultra_compact, request.size);
    let _ = write_settings_file(app, &settings);
}

fn start_window_size_writer(
    app: AppHandle,
    receiver: mpsc::Receiver<WindowSizeSaveRequest>,
    tracker: WindowModeTracker,
) {
    let _ = thread::Builder::new()
        .name("window-size-persistence".into())
        .spawn(move || {
            let mut pending = None;
            loop {
                if pending.is_some() {
                    match receiver.recv_timeout(RESIZE_SAVE_DEBOUNCE) {
                        Ok(request) => pending = Some(request),
                        Err(mpsc::RecvTimeoutError::Timeout) => {
                            if let Some(request) = pending.take() {
                                persist_window_size_request(&app, &tracker, request);
                            }
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => {
                            if let Some(request) = pending.take() {
                                persist_window_size_request(&app, &tracker, request);
                            }
                            break;
                        }
                    }
                } else {
                    match receiver.recv() {
                        Ok(request) => pending = Some(request),
                        Err(_) => break,
                    }
                }
            }
        });
}

const DIGIT_FIVE: [u8; 7] = [
    0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110,
];
const DIGIT_SEVEN: [u8; 7] = [
    0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
];
const LETTER_H: [u8; 7] = [
    0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
];
const LETTER_D: [u8; 7] = [
    0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110,
];

const TRAY_NEUTRAL_COLOR: [u8; 3] = [105, 107, 116];

fn blend_color(from: [u8; 3], to: [u8; 3], amount: f64) -> [u8; 3] {
    presentation::blend(from, to, amount)
}

fn tray_fill_color(remaining: f64, threshold: f64, position: f64) -> [u8; 3] {
    let palette = presentation::palette(remaining, threshold);
    if position <= 0.52 {
        blend_color(palette.deep, palette.mid, position / 0.52)
    } else {
        blend_color(palette.mid, palette.light, (position - 0.52) / 0.48)
    }
}

fn tray_fill_width(remaining: Option<f64>, is_fresh: bool) -> u32 {
    let Some(remaining) = remaining.filter(|value| value.is_finite()) else {
        return 0;
    };
    let bounded = remaining.clamp(0.0, 100.0);
    if !is_fresh || bounded == 0.0 {
        0
    } else {
        (((bounded / 100.0) * 24.0).round() as u32).max(1)
    }
}

fn set_icon_pixel(rgba: &mut [u8], x: u32, y: u32, color: [u8; 4]) {
    if x >= 32 || y >= 32 {
        return;
    }
    let index = ((y * 32 + x) * 4) as usize;
    rgba[index..index + 4].copy_from_slice(&color);
}

fn draw_icon_glyph(rgba: &mut [u8], x: u32, glyph: &[u8; 7], color: [u8; 4]) {
    for (row, bits) in glyph.iter().enumerate() {
        for column in 0..5u32 {
            if bits & (1 << (4 - column)) == 0 {
                continue;
            }
            for dy in 0..2u32 {
                for dx in 0..2u32 {
                    set_icon_pixel(rgba, x + column * 2 + dx, 2 + row as u32 * 2 + dy, color);
                }
            }
        }
    }
}

fn tray_icon(
    is_weekly: bool,
    remaining: Option<f64>,
    is_fresh: bool,
    threshold: f64,
) -> tauri::image::Image<'static> {
    let mut rgba = vec![0u8; 32 * 32 * 4];
    let glyph_color = [232, 235, 237, 255];
    let remaining = remaining
        .filter(|value| value.is_finite())
        .map(|value| value.clamp(0.0, 100.0));

    if is_weekly {
        draw_icon_glyph(&mut rgba, 4, &DIGIT_SEVEN, glyph_color);
        draw_icon_glyph(&mut rgba, 16, &LETTER_D, glyph_color);
    } else {
        draw_icon_glyph(&mut rgba, 4, &DIGIT_FIVE, glyph_color);
        draw_icon_glyph(&mut rgba, 16, &LETTER_H, glyph_color);
    }

    let track_color = [46, 50, 54, 255];
    for y in 22..27u32 {
        for x in 4..28u32 {
            set_icon_pixel(&mut rgba, x, y, track_color);
        }
    }
    let fill_width = tray_fill_width(remaining, is_fresh);
    for y in 22..27u32 {
        for x in 4..4 + fill_width {
            let color = match remaining.filter(|_| is_fresh) {
                Some(value) => {
                    let position =
                        f64::from(x - 4) / f64::from(fill_width.saturating_sub(1).max(1));
                    let mut blended = tray_fill_color(value, threshold, position);
                    if y == 22 {
                        blended = blend_color(blended, [255, 255, 255], 0.24);
                    } else if y == 26 {
                        blended = blend_color(blended, [0, 0, 0], 0.12);
                    }
                    [blended[0], blended[1], blended[2], 255]
                }
                None => [
                    TRAY_NEUTRAL_COLOR[0],
                    TRAY_NEUTRAL_COLOR[1],
                    TRAY_NEUTRAL_COLOR[2],
                    255,
                ],
            };
            set_icon_pixel(&mut rgba, x, y, color);
        }
    }
    tauri::image::Image::new_owned(rgba, 32, 32)
}

fn localized(language: Language, english: &'static str, spanish: &'static str) -> &'static str {
    match language {
        Language::En => english,
        Language::Es => spanish,
    }
}

fn tray_label(language: Language, is_weekly: bool) -> &'static str {
    if is_weekly {
        localized(language, "Weekly", "Semanal")
    } else {
        "5 h"
    }
}

fn tray_no_data(language: Language) -> &'static str {
    localized(language, "No data", "Sin datos")
}

fn tray_menu(app: &AppHandle, language: Language) -> tauri::Result<Menu<tauri::Wry>> {
    let open_item = MenuItem::with_id(
        app,
        "open",
        localized(language, "Show limits", "Mostrar límites"),
        true,
        None::<&str>,
    )?;
    let quit_item = MenuItem::with_id(
        app,
        "quit",
        localized(language, "Close…", "Cerrar…"),
        true,
        None::<&str>,
    )?;
    Menu::with_items(app, &[&open_item, &quit_item])
}

fn update_tray_language(app: &AppHandle, language: Language) {
    let runner = app.clone();
    let menu_app = app.clone();
    let _ = runner.run_on_main_thread(move || {
        for id in ["five-hour", "weekly"] {
            let Some(icon) = menu_app.tray_by_id(id) else {
                continue;
            };
            if let Ok(menu) = tray_menu(&menu_app, language) {
                let _ = icon.set_menu(Some(menu));
            }
        }
    });
}

fn handle_tray_menu_event(app: &AppHandle, event: tauri::menu::MenuEvent) {
    match event.id.as_ref() {
        "open" => show_panel(app),
        "quit" => request_close_confirmation(app),
        _ => {}
    }
}

fn request_close_confirmation(app: &AppHandle) {
    if app
        .try_state::<PanelReadyState>()
        .is_some_and(|ready| !ready.0.load(Ordering::Acquire))
    {
        return;
    }
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
        let _ = window.emit("close-requested", ());
    }
}

fn tooltip_text(value: String) -> Result<String, String> {
    if value.chars().any(char::is_control) {
        return Err("El texto de la bandeja no es válido.".into());
    }
    Ok(value.chars().take(127).collect())
}

#[tauri::command]
fn set_tray_tooltips(
    app: AppHandle,
    five_hour: String,
    weekly: String,
    revision: u64,
) -> Result<(), String> {
    if app
        .state::<AppRuntime>()
        .state
        .lock()
        .map_err(|_| "State unavailable")?
        .revision
        != revision
    {
        return Ok(());
    }
    let five_hour = tooltip_text(five_hour)?;
    let weekly = tooltip_text(weekly)?;
    let runner = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(icon) = runner.tray_by_id("five-hour") {
            let _ = icon.set_tooltip(Some(five_hour));
        }
        if let Some(icon) = runner.tray_by_id("weekly") {
            let _ = icon.set_tooltip(Some(weekly));
        }
    });
    Ok(())
}

pub(crate) fn update_tray_indicators(app: &AppHandle, state: &UsageState) {
    let snapshot = state.clone();
    let indicator_app = app.clone();
    let _ = app.run_on_main_thread(move || {
        if indicator_app
            .state::<AppRuntime>()
            .state
            .lock()
            .map(|state| state.revision > snapshot.revision)
            .unwrap_or(true)
        {
            return;
        }
        let settings = read_settings_file(&indicator_app);
        let is_fresh = matches!(snapshot.status.as_str(), "online" | "refreshing");
        for (id, is_weekly, window) in [
            ("five-hour", false, snapshot.five_hour.as_ref()),
            ("weekly", true, snapshot.weekly.as_ref()),
        ] {
            let remaining = window
                .filter(|_| is_fresh)
                .map(|window| window.remaining_fraction * 100.0);
            if let Some(icon) = indicator_app.tray_by_id(id) {
                let _ = icon.set_icon(Some(tray_icon(
                    is_weekly,
                    remaining,
                    is_fresh && remaining.is_some(),
                    if is_weekly {
                        settings.threshold_weekly
                    } else {
                        settings.threshold_5h
                    },
                )));
            }
        }
    });
}

fn refresh_tray_indicators(app: &AppHandle) {
    if let Some(runtime) = app.try_state::<AppRuntime>() {
        let snapshot = runtime
            .state
            .lock()
            .map(|state| state.clone())
            .unwrap_or_default();
        update_tray_indicators(app, &snapshot);
    }
}

fn primary_tray_click(event: TrayIconEvent) -> Option<tauri::PhysicalPosition<f64>> {
    match event {
        TrayIconEvent::Click {
            position,
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        } => Some(position),
        _ => None,
    }
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_panel(app);
        }))
        .setup(|app| {
            let initial_settings = read_settings_file(app.handle());
            let initial_language = initial_settings.language;
            app.manage(SettingsFileLock(Arc::new(Mutex::new(()))));
            app.manage(LanguageState(Mutex::new(initial_language)));
            let window_mode_tracker = WindowModeTracker(Arc::new(Mutex::new(WindowModeSnapshot {
                ultra_compact: false,
                revision: 0,
                suppress_until: Instant::now() + RESIZE_EVENT_SUPPRESSION,
                settings_height: None,
            })));
            app.manage(window_mode_tracker.clone());
            app.manage(PanelReadyState(AtomicBool::new(false)));
            let (window_size_tx, window_size_rx) = mpsc::channel();
            start_window_size_writer(
                app.handle().clone(),
                window_size_rx,
                window_mode_tracker.clone(),
            );
            app.manage(WindowSizeSaveChannel(window_size_tx.clone()));
            let initial_state = Arc::new(Mutex::new(UsageState::default()));
            let (command_tx, command_rx) = mpsc::channel();
            app.manage(AppRuntime {
                commands: command_tx.clone(),
                state: initial_state.clone(),
            });
            let five_hour_menu = tray_menu(app.handle(), initial_language)?;
            let five_hour_app = app.handle().clone();
            let _five_hour_tray = TrayIconBuilder::with_id("five-hour")
                .icon(tray_icon(false, None, false, initial_settings.threshold_5h))
                .tooltip(format!("5 h: {}", tray_no_data(initial_language)))
                .menu(&five_hour_menu)
                .show_menu_on_left_click(false)
                .on_menu_event(handle_tray_menu_event)
                .on_tray_icon_event(move |_tray, event| {
                    if primary_tray_click(event).is_some() {
                        show_panel(&five_hour_app);
                    }
                })
                .build(app)?;

            let weekly_menu = tray_menu(app.handle(), initial_language)?;
            let weekly_app = app.handle().clone();
            let _weekly_tray = TrayIconBuilder::with_id("weekly")
                .icon(tray_icon(
                    true,
                    None,
                    false,
                    initial_settings.threshold_weekly,
                ))
                .tooltip(format!(
                    "{}: {}",
                    tray_label(initial_language, true),
                    tray_no_data(initial_language)
                ))
                .menu(&weekly_menu)
                .show_menu_on_left_click(false)
                .on_menu_event(handle_tray_menu_event)
                .on_tray_icon_event(move |_tray, event| {
                    if primary_tray_click(event).is_some() {
                        show_panel(&weekly_app);
                    }
                })
                .build(app)?;

            backend::start_backend(app.handle().clone(), command_rx, initial_state)?;

            if let Some(window) = app.get_webview_window("main") {
                #[cfg(windows)]
                windows_native::install(&window).map_err(std::io::Error::other)?;
                if let Some(icon) = app.default_window_icon() {
                    let _ = window.set_icon(icon.clone());
                }
                let size = window_size_for(&initial_settings, initial_settings.ultra_compact);
                let initial_mode_applied =
                    set_window_size_for_mode(&window, initial_settings.ultra_compact, size).is_ok();
                if !initial_mode_applied {
                    let normal_size =
                        normalized_window_size(window_size_for(&initial_settings, false), false);
                    let _ = set_window_size_for_mode(&window, false, normal_size);
                }
                if let Ok(mut mode) = window_mode_tracker.0.lock() {
                    mode.ultra_compact = initial_mode_applied && initial_settings.ultra_compact;
                }
                let _ = window.set_always_on_top(initial_settings.always_on_top);
                let _ = window.set_skip_taskbar(true);
                let close_target = window.clone();
                let resize_target = window.clone();
                let resize_writer = window_size_tx.clone();
                let resize_tracker = window_mode_tracker.clone();
                window.on_window_event(move |event| match event {
                    WindowEvent::CloseRequested { api, .. } => {
                        api.prevent_close();
                        let _ = close_target.emit("close-requested", ());
                    }
                    WindowEvent::Resized(size) => {
                        let scale_factor = resize_target.scale_factor().unwrap_or(1.0);
                        if let Some(size) =
                            WindowSize::from_physical(size.width, size.height, scale_factor)
                        {
                            queue_window_size_save(&resize_writer, &resize_tracker, size);
                        }
                    }
                    WindowEvent::Focused(false) => {
                        queue_current_window_size_save(
                            &resize_target,
                            &resize_writer,
                            &resize_tracker,
                        );
                    }
                    _ => {}
                });
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_usage_state,
            get_app_version,
            get_native_window_mode,
            show_main_panel,
            set_ultra_compact,
            fit_settings_window,
            load_settings,
            save_settings,
            minimize_panel,
            set_always_on_top,
            set_tray_tooltips,
            quit_app
        ])
        .run(tauri::generate_context!())
        .expect("falló la aplicación de límites");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_version_is_exposed_to_the_main_window() {
        assert_eq!(get_app_version(), env!("CARGO_PKG_VERSION"));

        let permissions = include_str!("../permissions/usage.toml");
        assert!(
            permissions
                .lines()
                .any(|line| line.trim() == "\"get_app_version\","),
            "get_app_version must be allowed by the main window capability"
        );
    }

    #[test]
    fn threshold_defaults_and_bounds_are_explicit() {
        let defaults = Settings::default();
        assert_eq!(defaults.threshold_5h, 20.0);
        assert_eq!(defaults.threshold_weekly, 20.0);
        assert_eq!(defaults.ui_scale, 100);
        assert!(defaults.is_valid());
        assert!(!Settings {
            threshold_5h: 100.1,
            threshold_weekly: 20.0,
            language: Language::En,
            ui_scale: 100,
            always_on_top: true,
            close_action: CloseAction::Ask,
            ultra_compact: false,
            normal_window_size: default_normal_window_size(),
            ultra_compact_window_size: default_ultra_compact_window_size(),
        }
        .is_valid());
        assert!(!Settings {
            threshold_5h: f64::NAN,
            threshold_weekly: 20.0,
            language: Language::En,
            ui_scale: 100,
            always_on_top: true,
            close_action: CloseAction::Ask,
            ultra_compact: false,
            normal_window_size: default_normal_window_size(),
            ultra_compact_window_size: default_ultra_compact_window_size(),
        }
        .is_valid());
        assert!(!Settings {
            threshold_5h: 20.0,
            threshold_weekly: 20.0,
            language: Language::En,
            ui_scale: 105,
            always_on_top: true,
            close_action: CloseAction::Ask,
            ultra_compact: false,
            normal_window_size: default_normal_window_size(),
            ultra_compact_window_size: default_ultra_compact_window_size(),
        }
        .is_valid());
    }

    #[test]
    fn settings_json_uses_the_frontend_contract() {
        let encoded = serde_json::to_value(Settings::default()).unwrap();
        assert_eq!(
            encoded,
            serde_json::json!({
                "threshold5h": 20.0,
                "thresholdWeekly": 20.0,
                "language": "en",
                "uiScale": 100,
                "alwaysOnTop": true,
                "closeAction": "ask",
                "ultraCompact": false,
                "normalWindowSize": { "width": 360.0, "height": 220.0 },
                "ultraCompactWindowSize": { "width": 300.0, "height": 156.0 }
            })
        );
        let decoded: Settings = serde_json::from_value(encoded).unwrap();
        assert_eq!(decoded.threshold_5h, 20.0);
        assert_eq!(decoded.ui_scale, 100);
        assert_eq!(decoded.language, Language::En);
        assert!(decoded.always_on_top);
        assert!(!decoded.ultra_compact);
        assert_eq!(decoded.normal_window_size, default_normal_window_size());
        assert_eq!(
            decoded.ultra_compact_window_size,
            default_ultra_compact_window_size()
        );
    }

    #[test]
    fn existing_settings_without_scale_keep_their_thresholds() {
        let old_settings = serde_json::json!({
            "threshold5h": 15.5,
            "thresholdWeekly": 8.0
        });
        let decoded: Settings = serde_json::from_value(old_settings).unwrap();
        assert_eq!(decoded.threshold_5h, 15.5);
        assert_eq!(decoded.threshold_weekly, 8.0);
        assert_eq!(decoded.ui_scale, 100);
        assert!(decoded.always_on_top);
        assert_eq!(decoded.language, Language::En);
        assert!(!decoded.ultra_compact);
        assert_eq!(decoded.normal_window_size, default_normal_window_size());
        assert_eq!(
            decoded.ultra_compact_window_size,
            default_ultra_compact_window_size()
        );
    }

    #[test]
    fn compact_and_normal_window_sizes_are_distinct_and_bounded() {
        assert_eq!(window_mode_sizes(true), ((300.0, 156.0), (300.0, 156.0)));
        assert_eq!(
            window_mode_sizes(false),
            ((320.0, 220.0), NORMAL_WINDOW_SIZE)
        );
        assert_eq!(
            window_maximum(true),
            Some(WindowSize {
                width: MAX_WINDOW_DIMENSION,
                height: ULTRA_COMPACT_WINDOW_SIZE.1,
            })
        );
        assert_eq!(
            window_maximum(false),
            Some(WindowSize {
                width: MAX_WINDOW_DIMENSION,
                height: NORMAL_WINDOW_SIZE.1,
            })
        );
    }

    #[test]
    fn compact_size_keeps_its_width_but_forces_the_fixed_height() {
        let resized = WindowSize {
            width: 420.0,
            height: 410.0,
        };

        assert!(!resized.is_valid_for(true));
        assert_eq!(
            normalized_window_size(resized, true),
            WindowSize {
                width: 420.0,
                height: ULTRA_COMPACT_WINDOW_SIZE.1,
            }
        );
    }

    #[test]
    fn normal_size_keeps_its_width_and_discards_saved_vertical_slack() {
        let resized = WindowSize {
            width: 420.0,
            height: 410.0,
        };

        assert!(!resized.is_valid_for(false));
        assert_eq!(
            normalized_window_size(resized, false),
            WindowSize {
                width: 420.0,
                height: NORMAL_WINDOW_SIZE.1,
            }
        );
    }

    #[test]
    fn native_window_resize_runs_without_holding_the_mode_lock() {
        let tracker = WindowModeTracker(Arc::new(Mutex::new(WindowModeSnapshot {
            ultra_compact: true,
            revision: 4,
            suppress_until: Instant::now(),
            settings_height: None,
        })));

        let result = run_window_mode_transition(&tracker, false, || {
            let mode = tracker.0.try_lock().expect("mode lock must be released");
            assert!(!mode.ultra_compact);
            Ok(())
        });

        assert!(result.is_ok());
        assert!(!tracker.0.lock().unwrap().ultra_compact);
    }

    #[test]
    fn failed_native_resize_restores_the_previous_window_mode() {
        let tracker = WindowModeTracker(Arc::new(Mutex::new(WindowModeSnapshot {
            ultra_compact: true,
            revision: 4,
            suppress_until: Instant::now(),
            settings_height: None,
        })));

        let result = run_window_mode_transition(&tracker, false, || {
            Err::<(), _>("resize failed".to_owned())
        });

        assert_eq!(result, Err("resize failed".to_owned()));
        assert!(tracker.0.lock().unwrap().ultra_compact);
    }

    #[test]
    fn window_sizes_are_independent_and_restore_as_logical_dimensions() {
        let mut settings = Settings::default();
        let normal = WindowSize {
            width: 400.0,
            height: NORMAL_WINDOW_SIZE.1,
        };
        let compact = WindowSize {
            width: 320.0,
            height: ULTRA_COMPACT_WINDOW_SIZE.1,
        };
        set_window_size_for(&mut settings, false, normal);
        set_window_size_for(&mut settings, true, compact);

        assert_eq!(window_size_for(&settings, false), normal);
        assert_eq!(window_size_for(&settings, true), compact);
        assert!(normalized_window_size(normal, false).is_valid_for(false));
        assert!(normalized_window_size(compact, true).is_valid_for(true));
        for scale_factor in [1.0, 1.25, 1.5, 2.0] {
            assert_eq!(
                WindowSize::from_physical(
                    (normal.width * scale_factor) as u32,
                    (normal.height * scale_factor) as u32,
                    scale_factor,
                ),
                Some(normal)
            );
        }
        let restored: Settings =
            serde_json::from_value(serde_json::to_value(&settings).unwrap()).unwrap();
        assert_eq!(window_size_for(&restored, false), normal);
        assert_eq!(window_size_for(&restored, true), compact);
        assert_eq!(
            normalized_window_size(
                WindowSize {
                    width: 250.0,
                    height: 120.0,
                },
                true
            ),
            default_ultra_compact_window_size()
        );
    }

    #[test]
    fn tray_colors_follow_each_threshold_and_shared_palette() {
        for threshold in [0.0, 10.0, 20.0, 80.0, 100.0] {
            for remaining in [0.0, 5.0, 10.0, 20.0, 25.0, 25.01, 50.0, 84.0, 100.0] {
                assert_eq!(
                    tray_fill_color(remaining, threshold, 0.52),
                    presentation::palette(remaining, threshold).mid
                );
            }
        }
    }

    #[test]
    fn invalid_legacy_field_preserves_close_choice_and_other_preferences() {
        let settings = decode_settings(serde_json::json!({ "uiScale": 123, "threshold5h": 17, "closeAction": "minimize", "ultraCompact": true, "futureField": 1 })).unwrap();
        assert!(matches!(settings.close_action, CloseAction::Minimize));
        assert!(settings.ultra_compact);
        assert_eq!(settings.threshold_5h, 17.0);
        assert_eq!(settings.ui_scale, 100);
    }

    #[test]
    fn atomic_preferences_replace_existing_file_and_preserve_valid_json() {
        let directory = std::env::temp_dir().join(format!(
            "mystats-settings-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = directory.join("settings.json");
        let mut settings = Settings::default();
        write_settings_path(&path, &settings).unwrap();
        settings.close_action = CloseAction::Minimize;
        write_settings_path(&path, &settings).unwrap();
        let restored =
            decode_settings(serde_json::from_slice(&fs::read(&path).unwrap()).unwrap()).unwrap();
        assert!(matches!(restored.close_action, CloseAction::Minimize));
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 1);
        fs::remove_file(path).unwrap();
        fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn tray_bar_width_matches_zero_and_small_percentages() {
        assert_eq!(tray_fill_width(Some(0.0), true), 0);
        assert_eq!(tray_fill_width(Some(1.0), true), 1);
        assert_eq!(tray_fill_width(Some(100.0), true), 24);
        assert_eq!(tray_fill_width(Some(50.0), false), 0);
        assert_eq!(tray_fill_width(None, true), 0);
        assert_eq!(tray_fill_width(Some(f64::NAN), true), 0);
    }
}
