use std::fs;
use std::path::{Path, PathBuf};

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use uuid::Uuid;

use crate::models::{
    TimelineFramesRequest, TimelineFramesResult, TrimRequest, TrimResult, TrimSelection,
};
use crate::services::{dependencies, file_actions, process, storage};

type TrimServiceResult<T> = Result<T, String>;

const MIN_TRIM_DURATION: f64 = 0.25;
const TIMELINE_FRAME_WIDTH: u32 = 160;
const DEFAULT_TIMELINE_FRAME_COUNT: u32 = 12;
const MAX_TIMELINE_FRAME_COUNT: u32 = 24;

pub fn export_trim(request: TrimRequest) -> TrimServiceResult<TrimResult> {
    let source = existing_media_file(&request.source_path)?;
    let selection = validated_selection(&request.selection)?;
    let output = save_output_path(&source, &selection)?;

    run_export(&source, &selection, &output)?;

    Ok(TrimResult {
        output_path: path_to_string(output),
    })
}

pub fn copy_trim(request: TrimRequest) -> TrimServiceResult<TrimResult> {
    let source = existing_media_file(&request.source_path)?;
    let selection = validated_selection(&request.selection)?;
    let output = temporary_output_path()?;

    run_export(&source, &selection, &output)?;
    file_actions::copy_file_path(&path_to_string(&output))?;

    Ok(TrimResult {
        output_path: path_to_string(output),
    })
}

pub fn generate_timeline_frames(
    request: TimelineFramesRequest,
) -> TrimServiceResult<TimelineFramesResult> {
    let source = existing_media_file(&request.source_path)?;
    if !request.duration.is_finite() || request.duration <= 0.0 {
        return Err("Video duration is not available.".to_string());
    }

    let frame_count = request
        .frame_count
        .unwrap_or(DEFAULT_TIMELINE_FRAME_COUNT)
        .clamp(1, MAX_TIMELINE_FRAME_COUNT);
    let ffmpeg = dependencies::find_executable("ffmpeg")
        .ok_or_else(|| "ffmpeg was not found in PATH.".to_string())?;
    let frame_dir = storage::trim_exports_dir()?.join(format!("frames-{}", Uuid::new_v4()));
    fs::create_dir_all(&frame_dir)
        .map_err(|error| format!("Could not create timeline frame directory: {error}"))?;

    let mut frames = Vec::new();
    let mut first_error: Option<String> = None;

    for index in 0..frame_count {
        let timestamp = timeline_frame_timestamp(request.duration, index, frame_count);
        let output = frame_dir.join(format!("{index:02}.jpg"));
        let arguments = timeline_frame_arguments(&source, &output, timestamp);
        let process_output = process::run_hidden(&ffmpeg, &arguments, &dependencies::process_path())
            .map_err(|error| format!("Could not start ffmpeg: {error}"))?;

        if process_output.status.success() && output.is_file() {
            let data = fs::read(&output)
                .map_err(|error| format!("Could not read timeline frame: {error}"))?;
            frames.push(format!("data:image/jpeg;base64,{}", STANDARD.encode(data)));
        } else if first_error.is_none() {
            let stderr = String::from_utf8_lossy(&process_output.stderr)
                .trim()
                .to_string();
            first_error = Some(if stderr.is_empty() {
                "Could not generate timeline frame.".to_string()
            } else {
                stderr
            });
        }
    }

    let _ = fs::remove_dir_all(&frame_dir);

    if frames.is_empty() {
        Err(first_error.unwrap_or_else(|| "Could not generate timeline frames.".to_string()))
    } else {
        Ok(TimelineFramesResult { frames })
    }
}

pub fn export_arguments(source: &Path, selection: &TrimSelection, output: &Path) -> Vec<String> {
    vec![
        "-y".to_string(),
        "-i".to_string(),
        path_to_string(source),
        "-ss".to_string(),
        format_seconds(selection.start),
        "-t".to_string(),
        format_seconds(selection.end - selection.start),
        "-map".to_string(),
        "0:v:0".to_string(),
        "-map".to_string(),
        "0:a?".to_string(),
        "-c:v".to_string(),
        "libx264".to_string(),
        "-preset".to_string(),
        "veryfast".to_string(),
        "-crf".to_string(),
        "18".to_string(),
        "-pix_fmt".to_string(),
        "yuv420p".to_string(),
        "-c:a".to_string(),
        "aac".to_string(),
        "-b:a".to_string(),
        "192k".to_string(),
        "-movflags".to_string(),
        "+faststart".to_string(),
        path_to_string(output),
    ]
}

pub fn timeline_frame_arguments(source: &Path, output: &Path, timestamp: f64) -> Vec<String> {
    vec![
        "-y".to_string(),
        "-ss".to_string(),
        format_seconds(timestamp),
        "-i".to_string(),
        path_to_string(source),
        "-frames:v".to_string(),
        "1".to_string(),
        "-vf".to_string(),
        format!("scale={TIMELINE_FRAME_WIDTH}:-1"),
        "-q:v".to_string(),
        "4".to_string(),
        path_to_string(output),
    ]
}

fn run_export(source: &Path, selection: &TrimSelection, output: &Path) -> TrimServiceResult<()> {
    let ffmpeg = dependencies::find_executable("ffmpeg")
        .ok_or_else(|| "ffmpeg was not found in PATH.".to_string())?;

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Could not create trim output directory: {error}"))?;
    }

    if output.exists() {
        fs::remove_file(output)
            .map_err(|error| format!("Could not replace existing trim output: {error}"))?;
    }

    let arguments = export_arguments(source, selection, output);
    let process_output = process::run_hidden(ffmpeg, &arguments, &dependencies::process_path())
        .map_err(|error| format!("Could not start ffmpeg: {error}"))?;

    if process_output.status.success() && output.is_file() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&process_output.stderr)
        .trim()
        .to_string();
    Err(if stderr.is_empty() {
        "Trim export failed.".to_string()
    } else {
        stderr
    })
}

fn existing_media_file(path: &str) -> TrimServiceResult<PathBuf> {
    let path = PathBuf::from(path);
    if path.is_file() {
        Ok(path)
    } else {
        Err("Source media file does not exist.".to_string())
    }
}

fn validated_selection(selection: &TrimSelection) -> TrimServiceResult<TrimSelection> {
    if !selection.start.is_finite() || !selection.end.is_finite() {
        return Err("Trim range must use valid seconds.".to_string());
    }

    if selection.start < 0.0 {
        return Err("Trim start cannot be negative.".to_string());
    }

    if selection.end - selection.start < MIN_TRIM_DURATION {
        return Err("Choose a trim range of at least 0.25 seconds.".to_string());
    }

    Ok(selection.clone())
}

fn save_output_path(source: &Path, selection: &TrimSelection) -> TrimServiceResult<PathBuf> {
    let folder = source
        .parent()
        .ok_or_else(|| "Could not resolve source folder.".to_string())?;
    let stem = source
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("trimmed-video");
    let start = selection.start.round() as i64;
    let end = selection.end.round() as i64;

    Ok(folder.join(format!("{stem} trim {start}-{end}s.mp4")))
}

fn temporary_output_path() -> TrimServiceResult<PathBuf> {
    Ok(storage::trim_exports_dir()?.join(format!("{}.mp4", Uuid::new_v4())))
}

fn timeline_frame_timestamp(duration: f64, index: u32, count: u32) -> f64 {
    if count == 0 || !duration.is_finite() || duration <= 0.0 {
        return 0.0;
    }

    let fraction = (f64::from(index) + 0.5) / f64::from(count);
    (duration * fraction).clamp(0.0, (duration - 0.05).max(0.0))
}

fn format_seconds(value: f64) -> String {
    format!("{value:.3}")
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
    fn export_arguments_reencode_mp4_clip() {
        let source = Path::new("C:\\in.mp4");
        let output = Path::new("C:\\out.mp4");
        let selection = TrimSelection {
            start: 1.234,
            end: 3.456,
        };
        let args = export_arguments(source, &selection, output);

        assert!(contains_sequence(&args, &["-i", "C:\\in.mp4"]));
        assert!(contains_sequence(&args, &["-ss", "1.234"]));
        assert!(contains_sequence(&args, &["-t", "2.222"]));
        assert!(contains_sequence(&args, &["-c:v", "libx264"]));
        assert!(contains_sequence(&args, &["-pix_fmt", "yuv420p"]));
        assert!(!contains_sequence(&args, &["-c", "copy"]));
        assert_eq!(args.last().map(String::as_str), Some("C:\\out.mp4"));
    }

    #[test]
    fn validated_selection_rejects_tiny_ranges() {
        let selection = TrimSelection {
            start: 1.0,
            end: 1.1,
        };

        assert!(validated_selection(&selection).is_err());
    }

    #[test]
    fn timeline_frame_arguments_capture_scaled_jpeg() {
        let args = timeline_frame_arguments(Path::new("C:\\in.mp4"), Path::new("C:\\out.jpg"), 2.5);

        assert!(contains_sequence(&args, &["-ss", "2.500"]));
        assert!(contains_sequence(&args, &["-frames:v", "1"]));
        assert!(contains_sequence(&args, &["-vf", "scale=160:-1"]));
        assert_eq!(args.last().map(String::as_str), Some("C:\\out.jpg"));
    }

    #[test]
    fn timeline_timestamp_uses_center_of_each_slot() {
        assert_eq!(timeline_frame_timestamp(10.0, 0, 5), 1.0);
        assert_eq!(timeline_frame_timestamp(10.0, 4, 5), 9.0);
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
