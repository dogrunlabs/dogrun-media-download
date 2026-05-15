use std::fs;
use std::path::{Path, PathBuf};

use crate::models::{AppStateSnapshot, DownloadItem, Preferences};

const APP_DIR_NAME: &str = "DogRunMediaDownload";
const PREFERENCES_FILE: &str = "preferences.json";
const HISTORY_FILE: &str = "history.json";
const THUMBNAILS_DIR: &str = "thumbnails";
const TRIM_EXPORTS_DIR: &str = "trim-exports";

type StorageResult<T> = Result<T, String>;

pub fn app_state_snapshot() -> StorageResult<AppStateSnapshot> {
    let app_data_dir = ensure_app_data_dir()?;

    Ok(AppStateSnapshot {
        preferences: load_preferences()?,
        history: load_history()?,
        app_data_dir: path_to_string(&app_data_dir),
    })
}

pub fn load_preferences() -> StorageResult<Preferences> {
    let path = preferences_path()?;

    if !path.exists() {
        return save_preferences(default_preferences());
    }

    let contents = fs::read_to_string(&path)
        .map_err(|error| format!("Could not read preferences: {error}"))?;
    let preferences: Preferences = serde_json::from_str(&contents)
        .map_err(|error| format!("Could not parse preferences: {error}"))?;
    let original_preferences = preferences.clone();
    let preferences = repaired_preferences(preferences)?;

    if preferences != original_preferences {
        save_preferences(preferences.clone())?;
    }

    Ok(preferences)
}

pub fn save_preferences(preferences: Preferences) -> StorageResult<Preferences> {
    let preferences = repaired_preferences(preferences)?;
    let path = preferences_path()?;
    write_json(&path, &preferences)?;
    Ok(preferences)
}

pub fn load_history() -> StorageResult<Vec<DownloadItem>> {
    let path = history_path()?;

    if !path.exists() {
        save_history(Vec::new())?;
        return Ok(Vec::new());
    }

    let contents =
        fs::read_to_string(&path).map_err(|error| format!("Could not read history: {error}"))?;
    serde_json::from_str(&contents).map_err(|error| format!("Could not parse history: {error}"))
}

pub fn save_history(history: Vec<DownloadItem>) -> StorageResult<Vec<DownloadItem>> {
    let path = history_path()?;
    write_json(&path, &history)?;
    Ok(history)
}

pub fn clear_history() -> StorageResult<Vec<DownloadItem>> {
    save_history(Vec::new())
}

pub fn delete_history_item(id: &str) -> StorageResult<Vec<DownloadItem>> {
    let mut history = load_history()?;
    let original_len = history.len();
    history.retain(|item| item.id != id);

    if history.len() == original_len {
        return Err("History item was not found.".to_string());
    }

    save_history(history)
}

pub fn thumbnails_dir() -> StorageResult<PathBuf> {
    let directory = ensure_app_data_dir()?.join(THUMBNAILS_DIR);
    fs::create_dir_all(&directory)
        .map_err(|error| format!("Could not create thumbnails directory: {error}"))?;
    Ok(directory)
}

pub fn trim_exports_dir() -> StorageResult<PathBuf> {
    let directory = ensure_app_data_dir()?.join(TRIM_EXPORTS_DIR);
    fs::create_dir_all(&directory)
        .map_err(|error| format!("Could not create trim exports directory: {error}"))?;
    Ok(directory)
}

fn ensure_app_data_dir() -> StorageResult<PathBuf> {
    let directory = app_data_dir()?;
    fs::create_dir_all(&directory)
        .map_err(|error| format!("Could not create app data directory: {error}"))?;
    Ok(directory)
}

fn preferences_path() -> StorageResult<PathBuf> {
    Ok(ensure_app_data_dir()?.join(PREFERENCES_FILE))
}

fn history_path() -> StorageResult<PathBuf> {
    Ok(ensure_app_data_dir()?.join(HISTORY_FILE))
}

fn app_data_dir() -> StorageResult<PathBuf> {
    if let Some(app_data) = std::env::var_os("APPDATA") {
        return Ok(PathBuf::from(app_data).join(APP_DIR_NAME));
    }

    if let Some(user_profile) = std::env::var_os("USERPROFILE") {
        return Ok(PathBuf::from(user_profile)
            .join("AppData")
            .join("Roaming")
            .join(APP_DIR_NAME));
    }

    Err("Could not resolve Windows app data directory.".to_string())
}

fn default_preferences() -> Preferences {
    Preferences {
        download_folder: default_download_folder(),
        language: "system".to_string(),
        local_play_pause_shortcut: "Space".to_string(),
        global_activation_shortcut_enabled: false,
        global_activation_shortcut: "CommandOrControl+Shift+D".to_string(),
        update_check_enabled: false,
    }
}

fn repaired_preferences(mut preferences: Preferences) -> StorageResult<Preferences> {
    if preferences.download_folder.trim().is_empty() {
        preferences.download_folder = default_download_folder();
    }
    preferences.local_play_pause_shortcut =
        normalize_local_shortcut(&preferences.local_play_pause_shortcut);
    preferences.language = normalize_language(&preferences.language);
    preferences.global_activation_shortcut =
        normalize_global_shortcut(&preferences.global_activation_shortcut);

    let selected_folder = PathBuf::from(&preferences.download_folder);
    match ensure_download_folder(&selected_folder) {
        Ok(()) => {
            preferences.download_folder = path_to_string(selected_folder);
            Ok(preferences)
        }
        Err(selected_error) => {
            let fallback_folder = PathBuf::from(default_download_folder());
            ensure_download_folder(&fallback_folder).map_err(|fallback_error| {
                format!(
                    "Could not create download folder ({selected_error}) or default Downloads folder ({fallback_error})."
                )
            })?;

            Ok(Preferences {
                download_folder: path_to_string(fallback_folder),
                ..preferences
            })
        }
    }
}

fn normalize_language(value: &str) -> String {
    match value.trim() {
        "en" => "en".to_string(),
        "zh-CN" | "zh" | "zh-Hans" => "zh-CN".to_string(),
        "ja" => "ja".to_string(),
        "ko" => "ko".to_string(),
        "de" => "de".to_string(),
        "fr" => "fr".to_string(),
        "es" => "es".to_string(),
        _ => "system".to_string(),
    }
}

fn ensure_download_folder(folder: &Path) -> StorageResult<()> {
    if folder.as_os_str().is_empty() {
        return Err("Download folder cannot be empty.".to_string());
    }

    if folder.exists() && !folder.is_dir() {
        return Err("Download folder path is not a folder.".to_string());
    }

    fs::create_dir_all(folder).map_err(|error| format!("Could not create download folder: {error}"))
}

fn default_download_folder() -> String {
    if let Some(user_profile) = std::env::var_os("USERPROFILE") {
        return path_to_string(PathBuf::from(user_profile).join("Downloads"));
    }

    path_to_string(PathBuf::from(".").join("Downloads"))
}

fn normalize_local_shortcut(value: &str) -> String {
    match value.trim() {
        "Enter" => "Enter".to_string(),
        "P" | "p" => "P".to_string(),
        _ => "Space".to_string(),
    }
}

fn normalize_global_shortcut(value: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        return "CommandOrControl+Shift+D".to_string();
    }

    value.to_string()
}

fn write_json<T: serde::Serialize>(path: &Path, value: &T) -> StorageResult<()> {
    let parent = path
        .parent()
        .ok_or_else(|| "Could not resolve data file parent directory.".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("Could not create data directory: {error}"))?;

    let data = serde_json::to_string_pretty(value)
        .map_err(|error| format!("Could not serialize data: {error}"))?;
    fs::write(path, data).map_err(|error| format!("Could not write data file: {error}"))
}

fn path_to_string(path: impl AsRef<Path>) -> String {
    let path = path.as_ref();
    if let Ok(stripped) = path.strip_prefix(r"\\?\") {
        return stripped.to_string_lossy().to_string();
    }

    let text = path.to_string_lossy().to_string();
    text.strip_prefix(r"\\?\").unwrap_or(&text).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_preferences_uses_downloads_folder() {
        let preferences = default_preferences();

        assert!(preferences.download_folder.ends_with("Downloads"));
        assert!(!preferences.download_folder.trim().is_empty());
        assert_eq!(preferences.language, "system");
        assert_eq!(preferences.local_play_pause_shortcut, "Space");
        assert_eq!(
            preferences.global_activation_shortcut,
            "CommandOrControl+Shift+D"
        );
    }

    #[test]
    fn repaired_preferences_normalizes_language() {
        let mut preferences = default_preferences();
        preferences.language = "zh".to_string();

        let repaired = repaired_preferences(preferences).unwrap();

        assert_eq!(repaired.language, "zh-CN");
    }

    #[test]
    fn path_to_string_preserves_path_text() {
        let path = PathBuf::from("C:\\Users\\Example\\Downloads");

        assert_eq!(path_to_string(&path), "C:\\Users\\Example\\Downloads");
    }

    #[test]
    fn delete_history_item_removes_matching_id() {
        let mut history = vec![
            DownloadItem {
                id: "one".to_string(),
                source_url: "https://example.com/one".to_string(),
                title: "One".to_string(),
                file_path: "C:\\one.mp4".to_string(),
                thumbnail_path: None,
                created_at: "2026-01-01T00:00:00Z".to_string(),
            },
            DownloadItem {
                id: "two".to_string(),
                source_url: "https://example.com/two".to_string(),
                title: "Two".to_string(),
                file_path: "C:\\two.mp4".to_string(),
                thumbnail_path: None,
                created_at: "2026-01-01T00:00:00Z".to_string(),
            },
        ];

        history.retain(|item| item.id != "one");

        assert_eq!(history.len(), 1);
        assert_eq!(history[0].id, "two");
    }
}
