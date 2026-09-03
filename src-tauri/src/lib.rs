//! Desktop shell. Hosts the mind in-process, keeps it alive in the tray when
//! the window is closed, and exposes a thin command surface to the webview.
//! No business logic lives here; everything is a call into `aethra_core`.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use aethra_core::changes::ChangeRow;
use aethra_core::config::default_config_path;
use aethra_core::episodes::{EpisodeItem, EpisodeRow};
use aethra_core::identity::{Constitution, SelfModelSection};
use aethra_core::knowledge::{Note, Question, Summary};
use aethra_core::{AppConfig, ChatReply, Mind, MindStatus};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, RunEvent, State, WindowEvent};
use tauri_plugin_opener::OpenerExt;
use tokio::sync::{broadcast, watch};
use tracing_subscriber::fmt::writer::MakeWriterExt;

#[cfg(desktop)]
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
#[cfg(desktop)]
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

pub const EVENT_CHANNEL: &str = "aethra://event";

struct AppState {
    mind: Arc<Mind>,
    shutdown: watch::Sender<bool>,
}

type CmdResult<T> = Result<T, String>;

fn err_string(e: impl std::fmt::Display) -> String {
    e.to_string()
}

// --- commands ----------------------------------------------------------------

#[tauri::command]
async fn get_status(state: State<'_, AppState>) -> CmdResult<MindStatus> {
    state.mind.status().await.map_err(err_string)
}

#[tauri::command]
async fn chat_send(state: State<'_, AppState>, text: String) -> CmdResult<ChatReply> {
    state.mind.chat(&text).await.map_err(err_string)
}

#[tauri::command]
fn get_timeline(state: State<'_, AppState>, limit: u32, before: Option<String>) -> CmdResult<Vec<EpisodeRow>> {
    state.mind.timeline(limit, before.as_deref()).map_err(err_string)
}

#[tauri::command]
fn get_episode_items(state: State<'_, AppState>, episode_id: String) -> CmdResult<Vec<EpisodeItem>> {
    state.mind.episode_items(&episode_id).map_err(err_string)
}

#[tauri::command]
fn get_questions(state: State<'_, AppState>, status: Option<String>, limit: u32) -> CmdResult<Vec<Question>> {
    state.mind.questions(status.as_deref(), limit).map_err(err_string)
}

#[tauri::command]
fn add_question(state: State<'_, AppState>, text: String) -> CmdResult<Option<Question>> {
    state.mind.add_user_question(&text).map_err(err_string)
}

#[tauri::command]
fn retire_question(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    state.mind.retire_question(&id).map_err(err_string)
}

#[tauri::command]
fn get_notes(state: State<'_, AppState>, limit: u32) -> CmdResult<Vec<Note>> {
    state.mind.notes(limit).map_err(err_string)
}

#[tauri::command]
fn get_summaries(state: State<'_, AppState>, limit: u32) -> CmdResult<Vec<Summary>> {
    state.mind.summaries(limit).map_err(err_string)
}

#[tauri::command]
fn get_self_model(state: State<'_, AppState>) -> CmdResult<Vec<SelfModelSection>> {
    state.mind.self_model().map_err(err_string)
}

#[tauri::command]
fn get_constitution(state: State<'_, AppState>) -> CmdResult<Constitution> {
    state.mind.constitution().map_err(err_string)
}

#[tauri::command]
fn set_constitution(state: State<'_, AppState>, text: String) -> CmdResult<Constitution> {
    state.mind.set_constitution(&text).map_err(err_string)
}

#[tauri::command]
fn get_changes(state: State<'_, AppState>, limit: u32) -> CmdResult<Vec<ChangeRow>> {
    state.mind.changes(limit).map_err(err_string)
}

#[tauri::command]
fn request_learning(state: State<'_, AppState>) {
    state.mind.request_learning();
}

#[tauri::command]
fn stop_learning(state: State<'_, AppState>) {
    state.mind.stop_learning();
}

#[tauri::command]
fn touch_activity(state: State<'_, AppState>) {
    state.mind.touch_user_activity();
}

#[derive(Serialize)]
struct ConfigView {
    path: String,
    summary: BTreeMap<String, String>,
    /// Full effective configuration (secrets redacted) as JSON for display.
    json: String,
}

#[tauri::command]
fn get_config(state: State<'_, AppState>) -> CmdResult<ConfigView> {
    let redacted = state.mind.cfg.redacted();
    Ok(ConfigView {
        path: state.mind.config_path.display().to_string(),
        summary: redacted.summary(),
        json: serde_json::to_string_pretty(&redacted).map_err(err_string)?,
    })
}

#[tauri::command]
fn create_snapshot(state: State<'_, AppState>) -> CmdResult<Vec<String>> {
    state
        .mind
        .snapshot()
        .map(|paths| paths.iter().map(|p| p.display().to_string()).collect())
        .map_err(err_string)
}

#[tauri::command]
fn open_data_dir(app: AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
    let dir = state.mind.cfg.data_dir.display().to_string();
    app.opener().open_path(dir, None::<&str>).map_err(err_string)
}

#[tauri::command]
fn open_config_file(app: AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
    let path = state.mind.config_path.display().to_string();
    app.opener().open_path(path, None::<&str>).map_err(err_string)
}

// --- window and tray -----------------------------------------------------------

fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

/// The only exit path. `RunEvent::Exit` performs the actual mind shutdown.
fn quit(app: &AppHandle) {
    if let Some(state) = app.try_state::<AppState>() {
        let _ = state.shutdown.send(true);
    }
    app.exit(0);
}

/// Full process restart so `config.toml` is re-read. Goes through the same
/// `RunEvent::Exit` shutdown as Quit, then relaunches the binary.
///
/// Dev builds load the UI from Vite and the Tauri CLI stops Vite as soon as
/// the app exits, so a relaunched dev process would show a blank webview.
/// There we just quit and tell the user to re-run the dev command.
fn restart(app: &AppHandle) {
    if let Some(state) = app.try_state::<AppState>() {
        let _ = state.shutdown.send(true);
    }
    if tauri::is_dev() {
        tracing::info!("dev build: quitting instead of restarting; run `npm run tauri dev` again to reload config");
        app.exit(0);
    } else {
        app.request_restart();
    }
}

#[cfg(desktop)]
fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open Aethra", true, None::<&str>)?;
    let learn = MenuItem::with_id(app, "learn", "Start learning now", true, None::<&str>)?;
    let stop = MenuItem::with_id(app, "stop", "Stop learning", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let restart_label = if tauri::is_dev() { "Quit to reload config (dev)" } else { "Restart (reload config)" };
    let restart_item = MenuItem::with_id(app, "restart", restart_label, true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit (stops the mind)", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &learn, &stop, &sep, &restart_item, &quit_item])?;

    let mut builder = TrayIconBuilder::with_id("main")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip("Aethra")
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_main(app),
            "learn" => {
                if let Some(s) = app.try_state::<AppState>() {
                    s.mind.request_learning();
                }
                show_main(app);
            }
            "stop" => {
                if let Some(s) = app.try_state::<AppState>() {
                    s.mind.stop_learning();
                }
            }
            "restart" => restart(app),
            "quit" => quit(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

// --- startup -------------------------------------------------------------------

fn init_tracing(logs_dir: &Path) {
    let _ = std::fs::create_dir_all(logs_dir);
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,aethra_core=debug,project_aethra_lib=debug"));
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(logs_dir.join("aethra.log"));
    match file {
        Ok(f) => {
            let writer = std::io::stderr.and(Arc::new(f));
            tracing_subscriber::fmt()
                .with_env_filter(filter)
                .with_ansi(false)
                .with_writer(writer)
                .init();
        }
        Err(_) => {
            tracing_subscriber::fmt().with_env_filter(filter).init();
        }
    }
}

fn fail_startup(message: &str) -> ! {
    let dir = aethra_core::config::default_app_dir();
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(dir.join("startup-error.log"), message);
    eprintln!("{message}");
    std::process::exit(1)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let config_path = default_config_path();
    let cfg = match AppConfig::load_or_create(&config_path) {
        Ok(c) => c,
        Err(e) => fail_startup(&format!("Aethra could not load {}: {e}", config_path.display())),
    };
    init_tracing(&cfg.logs_dir());
    tracing::info!(config = %config_path.display(), data = %cfg.data_dir.display(), "starting Aethra");

    let builder = tauri::Builder::default();
    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
        show_main(app);
    }));

    builder
        .plugin(tauri_plugin_opener::init())
        .setup(move |app| {
            let mind = tauri::async_runtime::block_on(Mind::open(cfg, config_path))?;
            let (shutdown_tx, shutdown_rx) = watch::channel(false);
            app.manage(AppState {
                mind: mind.clone(),
                shutdown: shutdown_tx,
            });

            tauri::async_runtime::spawn(aethra_core::scheduler::run(mind.clone(), shutdown_rx));

            let handle = app.handle().clone();
            let mut events = mind.subscribe();
            tauri::async_runtime::spawn(async move {
                loop {
                    match events.recv().await {
                        Ok(ev) => {
                            let _ = handle.emit(EVENT_CHANNEL, &ev);
                        }
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            tracing::warn!("event bridge lagged by {n} events");
                        }
                        Err(broadcast::error::RecvError::Closed) => break,
                    }
                }
            });

            #[cfg(desktop)]
            build_tray(app.handle())?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                // Closing the window pauses the conversation, not the mind.
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_status,
            chat_send,
            get_timeline,
            get_episode_items,
            get_questions,
            add_question,
            retire_question,
            get_notes,
            get_summaries,
            get_self_model,
            get_constitution,
            set_constitution,
            get_changes,
            request_learning,
            stop_learning,
            touch_activity,
            get_config,
            create_snapshot,
            open_data_dir,
            open_config_file,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| match event {
            RunEvent::ExitRequested { api, code, .. } => {
                // Only the tray's Quit/Restart (app.exit / request_restart, both
                // carrying a code) may end the process. A codeless request comes
                // from the last window closing, which we treat as hide.
                if code.is_none() {
                    api.prevent_exit();
                }
            }
            RunEvent::Exit => {
                if let Some(state) = app.try_state::<AppState>() {
                    let _ = state.shutdown.send(true);
                    let mind = state.mind.clone();
                    tauri::async_runtime::block_on(async move { mind.shutdown().await });
                }
            }
            _ => {}
        });
}
