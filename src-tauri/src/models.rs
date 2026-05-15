use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolStatus {
    pub name: String,
    pub found: bool,
    pub path: Option<String>,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DependencyStatus {
    pub tools: Vec<ToolStatus>,
    pub missing_tools: Vec<String>,
    pub is_satisfied: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Preferences {
    pub download_folder: String,
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default = "default_local_play_pause_shortcut")]
    pub local_play_pause_shortcut: String,
    #[serde(default)]
    pub global_activation_shortcut_enabled: bool,
    #[serde(default = "default_global_activation_shortcut")]
    pub global_activation_shortcut: String,
    #[serde(default)]
    pub update_check_enabled: bool,
}

fn default_language() -> String {
    "system".to_string()
}

fn default_local_play_pause_shortcut() -> String {
    "Space".to_string()
}

fn default_global_activation_shortcut() -> String {
    "CommandOrControl+Shift+D".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadItem {
    pub id: String,
    pub source_url: String,
    pub title: String,
    pub file_path: String,
    pub thumbnail_path: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStateSnapshot {
    pub preferences: Preferences,
    pub history: Vec<DownloadItem>,
    pub app_data_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadRequest {
    pub source_url: String,
    pub destination_folder: Option<String>,
    pub request_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadResult {
    pub item: DownloadItem,
    pub history: Vec<DownloadItem>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgressEvent {
    pub request_id: Option<String>,
    pub source_url: String,
    pub status: String,
    pub percent: Option<f64>,
    pub speed: Option<String>,
    pub eta: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrimSelection {
    pub start: f64,
    pub end: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrimRequest {
    pub source_path: String,
    pub selection: TrimSelection,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrimResult {
    pub output_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineFramesRequest {
    pub source_path: String,
    pub duration: f64,
    pub frame_count: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineFramesResult {
    pub frames: Vec<String>,
}
