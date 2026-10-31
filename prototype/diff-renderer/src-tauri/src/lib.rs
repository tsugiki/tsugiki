// PROTOTYPE — throwaway. Tauri commands for the diff renderer spike.
mod model;

use model::{LoadInfo, Model, RowOut};
use std::sync::Mutex;

struct AppState(Mutex<Option<Model>>);

// Commands are async so heavy work runs off the main (UI) thread.
#[tauri::command]
async fn load(state: tauri::State<'_, AppState>, dataset: String, eager: bool) -> Result<LoadInfo, String> {
    let (model, info) = Model::load(&dataset, eager)?;
    *state.0.lock().unwrap() = Some(model);
    Ok(info)
}

#[tauri::command]
async fn all_rows(state: tauri::State<'_, AppState>) -> Result<Vec<RowOut>, String> {
    let mut guard = state.0.lock().unwrap();
    let model = guard.as_mut().ok_or("not loaded")?;
    let n = model.len();
    Ok(model.rows_out(0, n))
}

#[tauri::command]
async fn rows(state: tauri::State<'_, AppState>, start: usize, end: usize) -> Result<Vec<RowOut>, String> {
    let mut guard = state.0.lock().unwrap();
    let model = guard.as_mut().ok_or("not loaded")?;
    Ok(model.rows_out(start, end))
}

// Autobench: append a result line to $GHREV_PROTO_LOG (default bench-results.txt in the cwd).
#[tauri::command]
fn bench_log(line: String) -> Result<(), String> {
    use std::io::Write;
    let path = std::env::var("GHREV_PROTO_LOG").unwrap_or_else(|_| "bench-results.txt".into());
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path).map_err(|e| e.to_string())?;
    writeln!(f, "{line}").map_err(|e| e.to_string())
}

#[tauri::command]
fn autobench_requested() -> bool {
    std::env::var_os("GHREV_PROTO_AUTOBENCH").is_some()
}

#[tauri::command]
fn quit(app: tauri::AppHandle) {
    app.exit(0);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AppState(Mutex::new(None)))
        .invoke_handler(tauri::generate_handler![load, all_rows, rows, bench_log, autobench_requested, quit])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
