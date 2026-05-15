use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, SystemTime};

use tauri::Emitter;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::models::{DownloadItem, DownloadProgressEvent, DownloadRequest, DownloadResult};
use crate::services::{dependencies, process, storage, thumbnail};

type DownloadServiceResult<T> = Result<T, String>;

pub fn download_media_with_progress(
    request: DownloadRequest,
    app: Option<tauri::AppHandle>,
) -> DownloadServiceResult<DownloadResult> {
    let source_url = request.source_url.trim().to_string();
    validate_source_url(&source_url)?;

    let preferences = storage::load_preferences()?;
    let destination_folder = request
        .destination_folder
        .filter(|path| !path.trim().is_empty())
        .unwrap_or(preferences.download_folder);
    let destination_folder = PathBuf::from(destination_folder);
    fs::create_dir_all(&destination_folder)
        .map_err(|error| format!("Could not create download folder: {error}"))?;

    let yt_dlp = dependencies::find_executable("yt-dlp")
        .ok_or_else(|| "yt-dlp was not found in PATH.".to_string())?;
    if dependencies::find_executable("ffmpeg").is_none() {
        return Err("ffmpeg was not found in PATH.".to_string());
    }

    let started_at = SystemTime::now();
    let arguments = yt_dlp_arguments(&source_url, &destination_folder);
    let output = run_process_with_progress(
        &yt_dlp,
        &arguments,
        app.as_ref(),
        request.request_id.as_deref(),
        &source_url,
    )?;
    let (file_path, title) = parse_download_output(&output, &destination_folder, started_at)?;

    let item_id = Uuid::new_v4().to_string();
    let thumbnail_path = thumbnail::generate_thumbnail(&file_path, &item_id)
        .ok()
        .map(path_to_string);

    let item = DownloadItem {
        id: item_id,
        source_url,
        title,
        file_path: path_to_string(&file_path),
        thumbnail_path,
        created_at: OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string()),
    };

    let mut history = storage::load_history()?;
    history.insert(0, item.clone());
    let history = storage::save_history(history)?;

    Ok(DownloadResult { item, history })
}

pub fn yt_dlp_arguments(source_url: &str, destination_folder: &Path) -> Vec<String> {
    vec![
        "--no-playlist".to_string(),
        "--newline".to_string(),
        "--progress".to_string(),
        "--progress-delta".to_string(),
        "0.25".to_string(),
        "--progress-template".to_string(),
        "download:__MD_PROGRESS__%(progress.status)s|%(progress._percent_str)s|%(progress._speed_str)s|%(progress._eta_str)s".to_string(),
        "--restrict-filenames".to_string(),
        "--merge-output-format".to_string(),
        "mp4".to_string(),
        "--recode-video".to_string(),
        "mp4".to_string(),
        "-S".to_string(),
        "vcodec:h264,acodec:aac,ext:mp4:m4a".to_string(),
        "--paths".to_string(),
        path_to_string(destination_folder),
        "--output".to_string(),
        "%(title).180B [%(id)s].%(ext)s".to_string(),
        "--print".to_string(),
        "after_move:%(filepath)s".to_string(),
        "--print".to_string(),
        "after_move:%(title)s".to_string(),
        source_url.to_string(),
    ]
}

fn validate_source_url(source_url: &str) -> DownloadServiceResult<()> {
    let lower = source_url.to_ascii_lowercase();
    if (lower.starts_with("http://") || lower.starts_with("https://")) && lower.contains("://") {
        return Ok(());
    }

    Err("Enter a valid http or https URL.".to_string())
}

fn run_process_with_progress(
    executable: &Path,
    arguments: &[String],
    app: Option<&tauri::AppHandle>,
    request_id: Option<&str>,
    source_url: &str,
) -> DownloadServiceResult<String> {
    let mut command = process::hidden_piped_command(executable);
    command
        .args(arguments)
        .env("PATH", dependencies::process_path())
        .env("PYTHONUTF8", "1")
        .env("PYTHONIOENCODING", "utf-8");

    let mut child = command
        .spawn()
        .map_err(|error| format!("Could not start yt-dlp: {error}"))?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Could not capture yt-dlp output.".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "Could not capture yt-dlp errors.".to_string())?;

    let (line_sender, line_receiver) = mpsc::channel::<ProcessLine>();
    spawn_output_reader(stdout, OutputKind::Stdout, line_sender.clone());
    spawn_output_reader(stderr, OutputKind::Stderr, line_sender);

    let mut stdout_lines = Vec::new();
    let mut stderr_lines = Vec::new();
    let (status_sender, status_receiver) = mpsc::channel();
    thread::spawn(move || {
        let _ = status_sender.send(child.wait());
    });

    let status = loop {
        match line_receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(line) => collect_process_line(
                line,
                app,
                request_id,
                source_url,
                &mut stdout_lines,
                &mut stderr_lines,
            ),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                break status_receiver
                    .recv()
                    .map_err(|_| "Download process ended unexpectedly.".to_string())?
                    .map_err(|error| format!("Download process failed: {error}"))?;
            }
        }

        if let Ok(status) = status_receiver.try_recv() {
            while let Ok(line) = line_receiver.recv() {
                collect_process_line(
                    line,
                    app,
                    request_id,
                    source_url,
                    &mut stdout_lines,
                    &mut stderr_lines,
                );
            }
            break status.map_err(|error| format!("Download process failed: {error}"))?;
        }
    };

    if status.success() {
        emit_progress(app, request_id, source_url, "finished", Some(100.0), None, None);
        return Ok(stdout_lines.join("\n"));
    }

    let stderr = stderr_lines.join("\n").trim().to_string();
    if stderr.is_empty() {
        Err("Download failed.".to_string())
    } else {
        Err(stderr)
    }
}

fn collect_process_line(
    line: ProcessLine,
    app: Option<&tauri::AppHandle>,
    request_id: Option<&str>,
    source_url: &str,
    stdout_lines: &mut Vec<String>,
    stderr_lines: &mut Vec<String>,
) {
    match line.kind {
        OutputKind::Stdout => {
            if !handle_progress_line(&line.text, app, request_id, source_url) {
                stdout_lines.push(line.text);
            }
        }
        OutputKind::Stderr => {
            if !handle_progress_line(&line.text, app, request_id, source_url) {
                stderr_lines.push(line.text);
            }
        }
    }
}

fn spawn_output_reader<R: Read + Send + 'static>(
    reader: R,
    kind: OutputKind,
    sender: mpsc::Sender<ProcessLine>,
) {
    thread::spawn(move || {
        for line in BufReader::new(reader).lines().map_while(Result::ok) {
            if sender
                .send(ProcessLine {
                    kind,
                    text: line.trim().to_string(),
                })
                .is_err()
            {
                break;
            }
        }
    });
}

#[derive(Debug, Clone, Copy)]
enum OutputKind {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone)]
struct ProcessLine {
    kind: OutputKind,
    text: String,
}

fn handle_progress_line(
    line: &str,
    app: Option<&tauri::AppHandle>,
    request_id: Option<&str>,
    source_url: &str,
) -> bool {
    let Some(raw) = line.strip_prefix("__MD_PROGRESS__") else {
        return false;
    };

    let mut parts = raw.splitn(4, '|');
    let status = parts.next().unwrap_or_default().trim();
    let percent_text = parts.next().unwrap_or_default();
    let speed = clean_progress_text(parts.next().unwrap_or_default());
    let eta = clean_progress_text(parts.next().unwrap_or_default());
    let percent = parse_percent(percent_text);

    emit_progress(
        app,
        request_id,
        source_url,
        status,
        percent,
        speed,
        eta,
    );
    true
}

fn emit_progress(
    app: Option<&tauri::AppHandle>,
    request_id: Option<&str>,
    source_url: &str,
    status: &str,
    percent: Option<f64>,
    speed: Option<String>,
    eta: Option<String>,
) {
    if let Some(app) = app {
        let _ = app.emit(
            "download-progress",
            DownloadProgressEvent {
                request_id: request_id.map(ToOwned::to_owned),
                source_url: source_url.to_string(),
                status: status.to_string(),
                percent,
                speed,
                eta,
            },
        );
    }
}

fn parse_percent(value: &str) -> Option<f64> {
    let percent = value.trim().trim_end_matches('%').trim().parse::<f64>().ok()?;
    if percent.is_finite() {
        Some(percent.clamp(0.0, 100.0))
    } else {
        None
    }
}

fn clean_progress_text(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || value == "N/A" {
        None
    } else {
        Some(value.to_string())
    }
}

fn parse_download_output(
    output: &str,
    destination_folder: &Path,
    started_at: SystemTime,
) -> DownloadServiceResult<(PathBuf, String)> {
    let lines = output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();

    let file_path = lines
        .iter()
        .map(PathBuf::from)
        .find(|path| path.is_file())
        .or_else(|| newest_media_file(destination_folder, started_at));

    let file_path = file_path.ok_or_else(|| {
        "Download finished but no output file was found in the destination folder.".to_string()
    })?;

    let file_path_text = path_to_string(&file_path);
    let parsed_title = lines
        .iter()
        .rev()
        .find(|line| *line != &file_path_text && !Path::new(line.as_str()).is_file())
        .cloned();
    let title = match parsed_title {
        Some(value) if !looks_mojibake(&value) => value,
        _ => file_stem(&file_path),
    };

    Ok((file_path, title))
}

fn newest_media_file(folder: &Path, started_at: SystemTime) -> Option<PathBuf> {
    let entries = fs::read_dir(folder).ok()?;
    let mut candidates = Vec::new();

    for entry in entries.flatten() {
        let path = entry.path();
        if !is_media_file(&path) {
            continue;
        }

        let modified = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .ok()?;
        if modified < started_at {
            continue;
        }

        candidates.push((path, modified));
    }

    candidates
        .into_iter()
        .max_by_key(|(_, modified)| *modified)
        .map(|(path, _)| path)
}

fn is_media_file(path: &Path) -> bool {
    let Some(extension) = path.extension().and_then(|value| value.to_str()) else {
        return false;
    };

    matches!(
        extension.to_ascii_lowercase().as_str(),
        "mp4" | "m4v" | "mov"
    )
}

fn path_to_string(path: impl AsRef<Path>) -> String {
    let path = path.as_ref();
    if let Ok(stripped) = path.strip_prefix(r"\\?\") {
        return stripped.to_string_lossy().to_string();
    }

    let text = path.to_string_lossy().to_string();
    text.strip_prefix(r"\\?\").unwrap_or(&text).to_string()
}

fn file_stem(path: &Path) -> String {
    path.file_stem()
        .map(|value| value.to_string_lossy().to_string())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "Downloaded media".to_string())
}

fn looks_mojibake(value: &str) -> bool {
    value.contains("\u{951f}\u{65a4}\u{62f7}") || value.contains('\u{fffd}')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yt_dlp_arguments_match_expected_output_strategy() {
        let args = yt_dlp_arguments("https://example.com/video", Path::new("C:\\Downloads"));

        assert!(contains_sequence(&args, &["--merge-output-format", "mp4"]));
        assert!(contains_sequence(&args, &["--recode-video", "mp4"]));
        assert!(contains_sequence(&args, &["--paths", "C:\\Downloads"]));
        assert!(contains_sequence(
            &args,
            &["--print", "after_move:%(filepath)s"]
        ));
        assert!(contains_sequence(
            &args,
            &["--print", "after_move:%(title)s"]
        ));
        assert_eq!(
            args.last().map(String::as_str),
            Some("https://example.com/video")
        );
    }

    #[test]
    fn validate_source_url_rejects_non_web_urls() {
        assert!(validate_source_url("https://example.com").is_ok());
        assert!(validate_source_url("http://example.com").is_ok());
        assert!(validate_source_url("file:///C:/video.mp4").is_err());
        assert!(validate_source_url("not a url").is_err());
    }

    #[test]
    fn looks_mojibake_detects_replacement_patterns() {
        assert!(looks_mojibake("title\u{951f}\u{65a4}\u{62f7}text"));
        assert!(looks_mojibake("title\u{fffd}text"));
        assert!(!looks_mojibake("normal title"));
    }

    fn contains_sequence(arguments: &[String], expected: &[&str]) -> bool {
        arguments.windows(expected.len()).any(|window| {
            window
                .iter()
                .map(String::as_str)
                .eq(expected.iter().copied())
        })
    }
}
