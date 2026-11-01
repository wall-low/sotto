pub mod errors;
pub mod job;
pub mod tools;

use std::path::PathBuf;
use std::sync::Arc;
use std::thread;

use tauri::{AppHandle, Emitter, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

use job::{Control, Event, Request};

#[tauri::command]
fn tools_status() -> tools::Status {
    tools::status()
}

#[tauri::command]
fn install_tools(app: AppHandle) {
    thread::spawn(move || {
        let progress_app = app.clone();
        let result = tools::install(move |progress| {
            let _ = progress_app.emit("setup-progress", progress);
        });
        let _ = match result {
            Ok(()) => app.emit("setup-done", ()),
            Err(detail) => app.emit("setup-failed", errors::setup_failed(&detail)),
        };
    });
}

#[tauri::command]
fn default_folder() -> PathBuf {
    dirs::video_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(std::env::temp_dir)
        .join("Sotto")
}

#[tauri::command]
fn start_job(request: Request, app: AppHandle, control: State<'_, Arc<Control>>) {
    let control = Arc::clone(&control);
    control.reset();
    thread::spawn(move || {
        job::run(&request, &control, &|event: Event| {
            let _ = app.emit("job", event);
        });
    });
}

#[tauri::command]
fn cancel_job(control: State<'_, Arc<Control>>) {
    control.cancel();
}

#[tauri::command]
fn open_file(path: PathBuf, app: AppHandle) -> Result<(), String> {
    app.opener()
        .open_path(path.to_string_lossy(), None::<&str>)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn reveal_file(path: PathBuf, app: AppHandle) -> Result<(), String> {
    app.opener().reveal_item_in_dir(path).map_err(|error| error.to_string())
}

#[tauri::command]
async fn pick_folder(app: AppHandle) -> Option<PathBuf> {
    app.dialog().file().blocking_pick_folder()?.into_path().ok()
}

#[tauri::command]
async fn pick_video(app: AppHandle) -> Option<PathBuf> {
    app.dialog()
        .file()
        .add_filter("Видео", &["mp4", "m4v", "mov", "mkv", "webm", "avi"])
        .blocking_pick_file()?
        .into_path()
        .ok()
}

pub fn run() {
    thread::spawn(|| {
        let _ = tools::update_ytdlp_if_stale();
    });

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(Arc::new(Control::default()))
        .invoke_handler(tauri::generate_handler![
            tools_status,
            install_tools,
            default_folder,
            start_job,
            cancel_job,
            open_file,
            reveal_file,
            pick_folder,
            pick_video
        ])
        .run(tauri::generate_context!())
        .expect("не удалось запустить приложение");
}
