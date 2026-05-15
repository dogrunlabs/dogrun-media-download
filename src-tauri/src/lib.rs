mod commands;
mod models;
mod services;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            commands::check_dependencies,
            commands::get_app_state,
            commands::get_preferences,
            commands::save_preferences,
            commands::get_history,
            commands::clear_history,
            commands::delete_history_item,
            commands::copy_file,
            commands::reveal_file,
            commands::open_source_url,
            commands::thumbnail_data_url,
            commands::allow_media_preview,
            commands::export_trim,
            commands::copy_trim,
            commands::generate_timeline_frames,
            commands::download_media
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
