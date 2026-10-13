mod backend;
mod protocol;

use backend::{request_backend_shutdown, AppRuntime, UsageState};
use serde::{Deserialize, Deserializer, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};

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
}

struct LanguageState(Mutex<Language>);

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
fn load_settings(app: AppHandle) -> Settings {
    let Ok(path) = settings_path(&app) else {
        return Settings::default();
    };
    let Ok(contents) = fs::read_to_string(path) else {
        return Settings::default();
    };
    let Ok(mut settings) = serde_json::from_str::<Settings>(&contents) else {
        return Settings::default();
    };
    if (0.0..=100.0).contains(&settings.threshold_5h) {
        settings.threshold_5h = settings.threshold_5h.round();
    }
    if (0.0..=100.0).contains(&settings.threshold_weekly) {
        settings.threshold_weekly = settings.threshold_weekly.round();
    }
    if settings.is_valid() {
        settings
    } else {
        Settings::default()
    }
}

#[tauri::command]
fn save_settings(app: AppHandle, settings: Settings) -> Result<Settings, String> {
    if !settings.is_valid() {
        return Err("Los umbrales deben ser números enteros entre 0 y 100.".into());
    }
    let path =
        settings_path(&app).map_err(|_| "No se pudo guardar la configuración.".to_owned())?;
    let parent = path
        .parent()
        .ok_or_else(|| "No se pudo guardar la configuración.".to_owned())?;
    fs::create_dir_all(parent).map_err(|_| "No se pudo guardar la configuración.".to_owned())?;
    let serialized = serde_json::to_vec_pretty(&settings)
        .map_err(|_| "No se pudo guardar la configuración.".to_owned())?;
    fs::write(path, serialized).map_err(|_| "No se pudo guardar la configuración.".to_owned())?;
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
        refresh_tray_indicators(&app);
    }
    Ok(settings)
}

#[tauri::command]
fn get_app_version() -> String {
    env!("CARGO_PKG_VERSION").to_owned()
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
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn toggle_panel(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        if window.is_minimized().unwrap_or(false) {
            let _ = window.unminimize();
            let _ = window.show();
            let _ = window.set_focus();
        } else if window.is_visible().unwrap_or(false) {
            let _ = window.hide();
        } else {
            let _ = window.unminimize();
            let _ = window.show();
            let _ = window.set_focus();
        }
    }
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
) -> tauri::image::Image<'static> {
    let mut rgba = vec![0u8; 32 * 32 * 4];
    let glyph_color = [232, 235, 237, 255];
    let neutral_color = [117, 124, 130, 255];
    let remaining = remaining
        .filter(|value| value.is_finite())
        .map(|value| value.clamp(0.0, 100.0));
    let fill_color = if !is_fresh {
        neutral_color
    } else {
        match remaining.unwrap_or(0.0) {
            value if value > 50.0 => [88, 208, 162, 255],
            value if value > 25.0 => [217, 199, 92, 255],
            value if value > 10.0 => [233, 145, 80, 255],
            _ => [232, 105, 114, 255],
        }
    };

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
    let fill_width = if is_fresh && remaining.is_some() {
        (((remaining.unwrap_or_default() / 100.0) * 24.0).round() as u32).max(2)
    } else {
        0
    };
    for y in 22..27u32 {
        for x in 4..4 + fill_width {
            set_icon_pixel(&mut rgba, x, y, fill_color);
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

fn current_language(app: &AppHandle) -> Language {
    app.try_state::<LanguageState>()
        .and_then(|state| state.0.lock().ok().map(|language| *language))
        .unwrap_or_default()
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
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
        let _ = window.emit("close-requested", ());
    }
}

fn tray_window_status(
    window: Option<&crate::protocol::UsageWindow>,
    is_fresh: bool,
    language: Language,
) -> (Option<f64>, String) {
    match window {
        Some(window) if is_fresh => (
            Some(window.remaining_fraction * 100.0),
            match language {
                Language::En => format!("{}% remaining", window.remaining_percent),
                Language::Es => {
                    format!("{} % restante", window.remaining_percent.replace('.', ","))
                }
            },
        ),
        Some(_) => (
            None,
            localized(language, "Stale data", "Dato desactualizado").into(),
        ),
        None => (None, tray_no_data(language).into()),
    }
}

pub(crate) fn update_tray_indicators(app: &AppHandle, state: &UsageState) {
    let snapshot = state.clone();
    let language = current_language(app);
    let indicator_app = app.clone();
    let _ = app.run_on_main_thread(move || {
        let is_fresh = snapshot.status == "online";
        for (id, is_weekly, window) in [
            ("five-hour", false, snapshot.five_hour.as_ref()),
            ("weekly", true, snapshot.weekly.as_ref()),
        ] {
            let (remaining, detail) = tray_window_status(window, is_fresh, language);
            if let Some(icon) = indicator_app.tray_by_id(id) {
                let _ = icon.set_icon(Some(tray_icon(is_weekly, remaining, is_fresh)));
                let _ = icon.set_tooltip(Some(format!(
                    "{}: {detail}",
                    tray_label(language, is_weekly)
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

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_panel(app);
        }))
        .setup(|app| {
            let initial_language = load_settings(app.handle().clone()).language;
            app.manage(LanguageState(Mutex::new(initial_language)));
            let initial_state = Arc::new(Mutex::new(UsageState::default()));
            let (command_tx, command_rx) = mpsc::channel();
            app.manage(AppRuntime {
                commands: command_tx.clone(),
                state: initial_state.clone(),
            });
            let five_hour_menu = tray_menu(app.handle(), initial_language)?;
            let five_hour_app = app.handle().clone();
            let _five_hour_tray = TrayIconBuilder::with_id("five-hour")
                .icon(tray_icon(false, None, false))
                .tooltip(format!("5 h: {}", tray_no_data(initial_language)))
                .menu(&five_hour_menu)
                .show_menu_on_left_click(false)
                .on_menu_event(handle_tray_menu_event)
                .on_tray_icon_event(move |_tray, event| {
                    if matches!(
                        event,
                        TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            ..
                        }
                    ) {
                        toggle_panel(&five_hour_app);
                    }
                })
                .build(app)?;

            let weekly_menu = tray_menu(app.handle(), initial_language)?;
            let weekly_app = app.handle().clone();
            let _weekly_tray = TrayIconBuilder::with_id("weekly")
                .icon(tray_icon(true, None, false))
                .tooltip(format!(
                    "{}: {}",
                    tray_label(initial_language, true),
                    tray_no_data(initial_language)
                ))
                .menu(&weekly_menu)
                .show_menu_on_left_click(false)
                .on_menu_event(handle_tray_menu_event)
                .on_tray_icon_event(move |_tray, event| {
                    if matches!(
                        event,
                        TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            ..
                        }
                    ) {
                        toggle_panel(&weekly_app);
                    }
                })
                .build(app)?;

            backend::start_backend(app.handle().clone(), command_rx, initial_state)?;

            if let Some(window) = app.get_webview_window("main") {
                if let Some(icon) = app.default_window_icon() {
                    let _ = window.set_icon(icon.clone());
                }
                let _ = window.set_skip_taskbar(false);
                let close_target = window.clone();
                window.on_window_event(move |event| {
                    if let WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = close_target.emit("close-requested", ());
                    }
                });
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_usage_state,
            get_app_version,
            load_settings,
            save_settings,
            minimize_panel,
            set_always_on_top,
            quit_app
        ])
        .run(tauri::generate_context!())
        .expect("falló la aplicación de límites");
}

#[cfg(test)]
mod tests {
    use super::*;

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
        }
        .is_valid());
        assert!(!Settings {
            threshold_5h: f64::NAN,
            threshold_weekly: 20.0,
            language: Language::En,
            ui_scale: 100,
            always_on_top: true,
            close_action: CloseAction::Ask,
        }
        .is_valid());
        assert!(!Settings {
            threshold_5h: 20.0,
            threshold_weekly: 20.0,
            language: Language::En,
            ui_scale: 105,
            always_on_top: true,
            close_action: CloseAction::Ask,
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
                "closeAction": "ask"
            })
        );
        let decoded: Settings = serde_json::from_value(encoded).unwrap();
        assert_eq!(decoded.threshold_5h, 20.0);
        assert_eq!(decoded.ui_scale, 100);
        assert_eq!(decoded.language, Language::En);
        assert!(decoded.always_on_top);
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
    }
}
