use crate::models::{
    AppStateSnapshot, DependencyStatus, DownloadItem, DownloadRequest, DownloadResult, Preferences,
    TimelineFramesRequest, TimelineFramesResult, TrimRequest, TrimResult,
};
use crate::services::{dependencies, downloader, file_actions, storage, thumbnail, trim};

#[tauri::command]
pub fn check_dependencies() -> DependencyStatus {
    dependencies::check()
}

#[tauri::command]
pub fn get_app_state() -> Result<AppStateSnapshot, String> {
    storage::app_state_snapshot()
}

#[tauri::command]
pub fn get_preferences() -> Result<Preferences, String> {
    storage::load_preferences()
}

#[tauri::command]
pub fn save_preferences(preferences: Preferences) -> Result<Preferences, String> {
    storage::save_preferences(preferences)
}

#[tauri::command]
pub fn get_history() -> Result<Vec<DownloadItem>, String> {
    storage::load_history()
}

#[tauri::command]
pub fn clear_history() -> Result<Vec<DownloadItem>, String> {
    storage::clear_history()
}

#[tauri::command]
pub fn delete_history_item(id: String) -> Result<Vec<DownloadItem>, String> {
    storage::delete_history_item(&id)
}

#[tauri::command]
pub fn copy_file(file_path: String) -> Result<(), String> {
    file_actions::copy_file_path(&file_path)
}

#[tauri::command]
pub fn reveal_file(file_path: String) -> Result<(), String> {
    file_actions::reveal_file(&file_path)
}

#[tauri::command]
pub fn open_source_url(source_url: String) -> Result<(), String> {
    file_actions::open_url(&source_url)
}

#[tauri::command]
pub fn thumbnail_data_url(thumbnail_path: String) -> Result<String, String> {
    thumbnail::thumbnail_data_url(&thumbnail_path)
}

#[tauri::command]
pub fn allow_media_preview(app: tauri::AppHandle, file_path: String) -> Result<(), String> {
    use tauri::Manager;

    let path = std::path::PathBuf::from(&file_path);
    if !path.is_file() {
        return Err("Media file does not exist.".to_string());
    }

    app.asset_protocol_scope()
        .allow_file(&path)
        .map_err(|error| format!("Could not allow media preview path: {error}"))?;

    Ok(())
}

#[tauri::command]
pub async fn export_trim(request: TrimRequest) -> Result<TrimResult, String> {
    tauri::async_runtime::spawn_blocking(move || trim::export_trim(request))
        .await
        .map_err(|error| format!("Trim task failed: {error}"))?
}

#[tauri::command]
pub async fn copy_trim(request: TrimRequest) -> Result<TrimResult, String> {
    tauri::async_runtime::spawn_blocking(move || trim::copy_trim(request))
        .await
        .map_err(|error| format!("Trim task failed: {error}"))?
}

#[tauri::command]
pub async fn generate_timeline_frames(
    request: TimelineFramesRequest,
) -> Result<TimelineFramesResult, String> {
    tauri::async_runtime::spawn_blocking(move || trim::generate_timeline_frames(request))
        .await
        .map_err(|error| format!("Timeline frame task failed: {error}"))?
}

#[tauri::command]
pub async fn download_media(
    app: tauri::AppHandle,
    request: DownloadRequest,
) -> Result<DownloadResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        downloader::download_media_with_progress(request, Some(app))
    })
        .await
        .map_err(|error| format!("Download task failed: {error}"))?
}
