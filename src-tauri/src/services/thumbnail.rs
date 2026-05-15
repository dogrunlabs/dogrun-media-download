use std::fs;
use std::path::{Path, PathBuf};

use base64::engine::general_purpose::STANDARD;
use base64::Engine;

use crate::services::{dependencies, process, storage};

type ThumbnailResult<T> = Result<T, String>;

const THUMBNAIL_WIDTH: u32 = 240;
const THUMBNAIL_EXTENSION: &str = "jpg";

pub fn generate_thumbnail(source: &Path, item_id: &str) -> ThumbnailResult<PathBuf> {
    if !source.is_file() {
        return Err("Media file does not exist.".to_string());
    }

    let output = thumbnail_path(item_id)?;
    if output.is_file() {
        return Ok(output);
    }

    let ffmpeg = dependencies::find_executable("ffmpeg")
        .ok_or_else(|| "ffmpeg was not found in PATH.".to_string())?;
    let duration = video_duration_seconds(source).ok();
    let timestamp = thumbnail_timestamp(duration);
    let arguments = ffmpeg_arguments(source, &output, timestamp);

    let process_output = process::run_hidden(ffmpeg, &arguments, &dependencies::process_path())
        .map_err(|error| format!("Could not start ffmpeg: {error}"))?;

    if !process_output.status.success() {
        let stderr = String::from_utf8_lossy(&process_output.stderr)
            .trim()
            .to_string();
        return Err(if stderr.is_empty() {
            "Could not generate thumbnail.".to_string()
        } else {
            stderr
        });
    }

    if output.is_file() {
        Ok(output)
    } else {
        Err("ffmpeg finished but no thumbnail was created.".to_string())
    }
}

pub fn thumbnail_path(item_id: &str) -> ThumbnailResult<PathBuf> {
    let file_name = format!(
        "{}.{THUMBNAIL_EXTENSION}",
        safe_thumbnail_stem(item_id).unwrap_or_else(|| "thumbnail".to_string())
    );
    Ok(storage::thumbnails_dir()?.join(file_name))
}

pub fn thumbnail_data_url(path: &str) -> ThumbnailResult<String> {
    let requested = PathBuf::from(path);
    let thumbnails_dir = storage::thumbnails_dir()?
        .canonicalize()
        .map_err(|error| format!("Could not resolve thumbnails directory: {error}"))?;
    let requested = requested
        .canonicalize()
        .map_err(|error| format!("Could not resolve thumbnail path: {error}"))?;

    if !requested.starts_with(&thumbnails_dir) {
        return Err("Thumbnail path is outside the app thumbnails directory.".to_string());
    }

    if !requested.is_file() || !is_jpeg(&requested) {
        return Err("Thumbnail file is not available.".to_string());
    }

    let data =
        fs::read(&requested).map_err(|error| format!("Could not read thumbnail: {error}"))?;
    Ok(format!("data:image/jpeg;base64,{}", STANDARD.encode(data)))
}

pub fn ffmpeg_arguments(source: &Path, output: &Path, timestamp: f64) -> Vec<String> {
    vec![
        "-y".to_string(),
        "-ss".to_string(),
        format_seconds(timestamp),
        "-i".to_string(),
        path_to_string(source),
        "-frames:v".to_string(),
        "1".to_string(),
        "-vf".to_string(),
        format!("scale={THUMBNAIL_WIDTH}:-1"),
        "-q:v".to_string(),
        "3".to_string(),
        path_to_string(output),
    ]
}

fn video_duration_seconds(source: &Path) -> ThumbnailResult<f64> {
    let ffprobe = dependencies::find_executable("ffprobe")
        .ok_or_else(|| "ffprobe was not found in PATH.".to_string())?;
    let arguments = vec![
        "-v".to_string(),
        "error".to_string(),
        "-show_entries".to_string(),
        "format=duration".to_string(),
        "-of".to_string(),
        "default=noprint_wrappers=1:nokey=1".to_string(),
        path_to_string(source),
    ];
    let output = process::run_hidden(ffprobe, &arguments, &dependencies::process_path())
        .map_err(|error| format!("Could not start ffprobe: {error}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            "Could not inspect media duration.".to_string()
        } else {
            stderr
        });
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .trim()
        .parse::<f64>()
        .map_err(|error| format!("Could not parse media duration: {error}"))
}

fn thumbnail_timestamp(duration: Option<f64>) -> f64 {
    match duration {
        Some(value) if value.is_finite() && value > 2.0 => 1.0,
        Some(value) if value.is_finite() && value > 0.5 => value / 2.0,
        _ => 0.0,
    }
}

fn safe_thumbnail_stem(value: &str) -> Option<String> {
    let stem = value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || *character == '-')
        .collect::<String>();

    if stem.is_empty() {
        None
    } else {
        Some(stem)
    }
}

fn is_jpeg(path: &Path) -> bool {
    let Some(extension) = path.extension().and_then(|value| value.to_str()) else {
        return false;
    };

    matches!(extension.to_ascii_lowercase().as_str(), "jpg" | "jpeg")
}

fn format_seconds(value: f64) -> String {
    if value <= 0.0 {
        "0".to_string()
    } else {
        format!("{value:.3}")
    }
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
    fn timestamp_uses_one_second_for_normal_videos() {
        assert_eq!(thumbnail_timestamp(Some(12.0)), 1.0);
    }

    #[test]
    fn timestamp_uses_middle_for_short_videos() {
        assert_eq!(thumbnail_timestamp(Some(1.0)), 0.5);
    }

    #[test]
    fn timestamp_falls_back_to_start_without_duration() {
        assert_eq!(thumbnail_timestamp(None), 0.0);
    }

    #[test]
    fn safe_thumbnail_stem_keeps_uuid_characters() {
        assert_eq!(
            safe_thumbnail_stem("f359c4d1-47b8-4ca1-a725-69c00b581886"),
            Some("f359c4d1-47b8-4ca1-a725-69c00b581886".to_string())
        );
    }

    #[test]
    fn ffmpeg_arguments_capture_single_scaled_frame() {
        let args = ffmpeg_arguments(Path::new("C:\\in.mp4"), Path::new("C:\\out.jpg"), 1.0);

        assert!(contains_sequence(&args, &["-ss", "1.000"]));
        assert!(contains_sequence(&args, &["-frames:v", "1"]));
        assert!(contains_sequence(&args, &["-vf", "scale=240:-1"]));
        assert_eq!(args.last().map(String::as_str), Some("C:\\out.jpg"));
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
