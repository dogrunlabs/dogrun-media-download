use std::env;
use std::path::{Component, Path, PathBuf};

use crate::models::{DependencyStatus, ToolStatus};

const REQUIRED_TOOLS: [&str; 3] = ["yt-dlp", "ffmpeg", "ffprobe"];

pub fn check() -> DependencyStatus {
    let tools: Vec<ToolStatus> = REQUIRED_TOOLS.iter().map(|tool| check_tool(tool)).collect();
    let missing_tools = tools
        .iter()
        .filter(|tool| !tool.found)
        .map(|tool| tool.name.clone())
        .collect::<Vec<_>>();

    DependencyStatus {
        is_satisfied: missing_tools.is_empty(),
        tools,
        missing_tools,
    }
}

fn check_tool(name: &str) -> ToolStatus {
    let path = find_executable(name);

    ToolStatus {
        name: name.to_string(),
        found: path.is_some(),
        path: path.map(path_to_string),
        version: None,
    }
}

pub fn find_executable(name: &str) -> Option<PathBuf> {
    if let Some(path) = find_bundled_executable(name) {
        return Some(path);
    }

    let candidates = executable_names(name);

    for directory in path_directories() {
        for candidate in &candidates {
            let path = directory.join(candidate);
            if is_executable_file(&path) {
                return Some(path);
            }
        }
    }

    None
}

pub fn process_path() -> String {
    env::join_paths(path_directories())
        .unwrap_or_default()
        .to_string_lossy()
        .to_string()
}

fn executable_names(name: &str) -> Vec<String> {
    let mut names = vec![name.to_string()];
    if !name.to_ascii_lowercase().ends_with(".exe") {
        names.push(format!("{name}.exe"));
    }
    names
}

fn find_bundled_executable(name: &str) -> Option<PathBuf> {
    let candidates = executable_names(name);
    for directory in bundled_tool_directories() {
        for candidate in &candidates {
            let path = directory.join(candidate);
            if is_executable_file(&path) {
                return Some(path);
            }
        }
    }

    None
}

fn bundled_tool_directories() -> Vec<PathBuf> {
    let mut directories = Vec::new();

    if let Ok(exe_path) = env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            directories.push(exe_dir.to_path_buf());
            directories.push(exe_dir.join("binaries"));
        }
    }

    if let Ok(resource_dir) = env::var("DOGRUN_MEDIA_DOWNLOAD_RESOURCES") {
        directories.push(PathBuf::from(resource_dir));
    }

    dedupe_directories(directories)
}

fn path_directories() -> Vec<PathBuf> {
    let mut directories = Vec::new();

    if let Some(path) = env::var_os("PATH") {
        directories.extend(env::split_paths(&path));
    }

    if let Some(user_profile) = env::var_os("USERPROFILE") {
        directories.push(PathBuf::from(&user_profile).join(".cargo").join("bin"));
        directories.push(
            PathBuf::from(&user_profile)
                .join("AppData")
                .join("Local")
                .join("Microsoft")
                .join("WindowsApps"),
        );
    }

    if let Some(local_app_data) = env::var_os("LOCALAPPDATA") {
        let winget_packages = PathBuf::from(local_app_data)
            .join("Microsoft")
            .join("WinGet")
            .join("Packages");
        append_winget_package_dirs(&mut directories, &winget_packages);
    }

    dedupe_directories(directories)
}

fn append_winget_package_dirs(directories: &mut Vec<PathBuf>, root: &Path) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        directories.push(path.clone());

        let bin = path.join("bin");
        if bin.is_dir() {
            directories.push(bin);
        }

        append_first_level_bin_dirs(directories, &path);
    }
}

fn append_first_level_bin_dirs(directories: &mut Vec<PathBuf>, package_dir: &Path) {
    let Ok(entries) = std::fs::read_dir(package_dir) else {
        return;
    };

    for entry in entries.flatten() {
        let child = entry.path();
        if !child.is_dir() {
            continue;
        }

        let bin = child.join("bin");
        if bin.is_dir() {
            directories.push(bin);
        }
    }
}

fn dedupe_directories(directories: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut result = Vec::new();

    for directory in directories {
        if directory.as_os_str().is_empty() || !directory.is_dir() {
            continue;
        }

        let normalized = directory
            .canonicalize()
            .unwrap_or_else(|_| directory.clone());

        if !result.iter().any(|existing| existing == &normalized) {
            result.push(normalized);
        }
    }

    result
}

fn is_executable_file(path: &Path) -> bool {
    path.is_file()
}

fn path_to_string(path: impl AsRef<Path>) -> String {
    let path = path.as_ref();
    let mut text = path.to_string_lossy().to_string();

    if let Ok(stripped) = path.strip_prefix(r"\\?\") {
        text = stripped.to_string_lossy().to_string();
    } else if matches!(path.components().next(), Some(Component::Prefix(_))) {
        text = text.strip_prefix(r"\\?\").unwrap_or(&text).to_string();
    }

    text
}
