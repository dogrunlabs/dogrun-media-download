use std::path::{Path, PathBuf};
use std::process::Command;

use crate::services::process;

type FileActionResult<T> = Result<T, String>;

pub fn copy_file_path(file_path: &str) -> FileActionResult<()> {
    let path = existing_file(file_path)?;
    let escaped = path_to_string(&path).replace('\'', "''");
    let script = format!("Set-Clipboard -LiteralPath '{}'", escaped);

    let output = process::hidden_command("powershell.exe")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &script,
        ])
        .output()
        .map_err(|error| format!("Could not start clipboard command: {error}"))?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(if stderr.is_empty() {
            "Could not copy file to clipboard.".to_string()
        } else {
            stderr
        })
    }
}

pub fn reveal_file(file_path: &str) -> FileActionResult<()> {
    let path = existing_file(file_path)?;
    Command::new("explorer.exe")
        .arg(format!("/select,{}", path_to_string(&path)))
        .spawn()
        .map_err(|error| format!("Could not open File Explorer: {error}"))?;
    Ok(())
}

pub fn open_url(source_url: &str) -> FileActionResult<()> {
    validate_web_url(source_url)?;
    Command::new("rundll32.exe")
        .args(["url.dll,FileProtocolHandler", source_url])
        .spawn()
        .map_err(|error| format!("Could not open source URL: {error}"))?;
    Ok(())
}

fn existing_file(file_path: &str) -> FileActionResult<PathBuf> {
    let path = PathBuf::from(file_path);
    if path.is_file() {
        Ok(path)
    } else {
        Err("File does not exist.".to_string())
    }
}

fn validate_web_url(source_url: &str) -> FileActionResult<()> {
    let lower = source_url.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        Ok(())
    } else {
        Err("Source URL is not a web URL.".to_string())
    }
}

fn path_to_string(path: impl AsRef<Path>) -> String {
    path.as_ref().to_string_lossy().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_web_url_accepts_http_and_https_only() {
        assert!(validate_web_url("https://example.com").is_ok());
        assert!(validate_web_url("http://example.com").is_ok());
        assert!(validate_web_url("file:///C:/video.mp4").is_err());
    }
}
