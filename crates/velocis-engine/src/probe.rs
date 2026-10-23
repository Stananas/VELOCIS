//! Media introspection via `ffprobe`.

use crate::model::{AssetKind, MediaAsset, Micros, ONE_SECOND};
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::path::Path;
use std::process::Command;

/// Paths to the external tools. Overridable so a packaged build can ship its own.
pub fn ffprobe_bin() -> String {
    std::env::var("VELOCIS_FFPROBE").unwrap_or_else(|_| "ffprobe".into())
}

pub fn ffmpeg_bin() -> String {
    std::env::var("VELOCIS_FFMPEG").unwrap_or_else(|_| "ffmpeg".into())
}

#[derive(Debug, Deserialize)]
struct ProbeOutput {
    #[serde(default)]
    streams: Vec<ProbeStream>,
    #[serde(default)]
    format: Option<ProbeFormat>,
}

#[derive(Debug, Deserialize)]
struct ProbeStream {
    #[serde(default)]
    codec_type: Option<String>,
    #[serde(default)]
    codec_name: Option<String>,
    #[serde(default)]
    width: Option<u32>,
    #[serde(default)]
    height: Option<u32>,
    #[serde(default)]
    channels: Option<u32>,
    #[serde(default)]
    sample_rate: Option<String>,
    /// `"30000/1001"` — exact rational frame rate.
    #[serde(default)]
    avg_frame_rate: Option<String>,
    #[serde(default)]
    r_frame_rate: Option<String>,
    /// Stream duration in *seconds* as a decimal string.
    #[serde(default)]
    duration: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ProbeFormat {
    #[serde(default)]
    duration: Option<String>,
    #[serde(default)]
    size: Option<String>,
}

/// Parse an ffprobe `a/b` rational into a float, tolerating junk.
pub fn parse_rational(s: &str) -> Option<f64> {
    let (n, d) = s.split_once('/')?;
    let n: f64 = n.trim().parse().ok()?;
    let d: f64 = d.trim().parse().ok()?;
    if d == 0.0 {
        return None;
    }
    Some(n / d)
}

fn parse_seconds(s: &str) -> Option<f64> {
    s.trim().parse::<f64>().ok()
}

/// Probe a file and build the asset record the project stores.
pub fn probe(path: &Path) -> Result<MediaAsset> {
    if !path.exists() {
        bail!("fichier introuvable: {}", path.display());
    }
    let out = Command::new(ffprobe_bin())
        .args([
            "-v",
            "error",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
        ])
        .arg(path)
        .output()
        .with_context(|| format!("exécution de {} impossible", ffprobe_bin()))?;

    if !out.status.success() {
        bail!(
            "ffprobe a échoué: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }

    let parsed: ProbeOutput =
        serde_json::from_slice(&out.stdout).context("sortie ffprobe illisible")?;

    let video = parsed
        .streams
        .iter()
        .find(|s| s.codec_type.as_deref() == Some("video"));

    // Cover art in an MP3 is a "video" stream; treat it as audio-only.
    let is_cover_art = is_cover_art_only(&parsed);

    let audio = parsed
        .streams
        .iter()
        .find(|s| s.codec_type.as_deref() == Some("audio"));

    let duration_secs = parsed
        .format
        .as_ref()
        .and_then(|f| f.duration.as_deref())
        .and_then(parse_seconds)
        .or_else(|| {
            parsed
                .streams
                .iter()
                .filter_map(|s| s.duration.as_deref())
                .filter_map(parse_seconds)
                .fold(None, |acc: Option<f64>, d| {
                    Some(acc.map_or(d, |a| a.max(d)))
                })
        })
        .unwrap_or(0.0);

    // Classification is by extension first: it is what the user actually picked,
    // and it sidesteps the fact that ffprobe reports a single frame of a PNG as
    // a video stream with a nominal frame rate.
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    const IMAGE_EXTS: [&str; 8] = ["png", "jpg", "jpeg", "webp", "bmp", "avif", "gif", "tiff"];

    let kind = if IMAGE_EXTS.contains(&ext.as_str()) {
        AssetKind::Image
    } else if video.is_some() && !is_cover_art {
        AssetKind::Video
    } else if audio.is_some() {
        AssetKind::Audio
    } else {
        bail!("aucune piste audio ou vidéo exploitable détectée");
    };

    // A still image has no meaningful duration: give the UI something to place.
    let duration: Micros = if kind == AssetKind::Image {
        5 * ONE_SECOND
    } else {
        (duration_secs * ONE_SECOND as f64).round() as Micros
    };

    let fps = video
        .and_then(|s| s.avg_frame_rate.as_deref())
        .or_else(|| video.and_then(|s| s.r_frame_rate.as_deref()))
        .and_then(parse_rational)
        .filter(|f| *f > 0.0)
        .unwrap_or(0.0);

    let size_bytes = parsed
        .format
        .as_ref()
        .and_then(|f| f.size.as_deref())
        .and_then(|s| s.trim().parse::<u64>().ok())
        .or_else(|| std::fs::metadata(path).ok().map(|m| m.len()))
        .unwrap_or(0);

    Ok(MediaAsset {
        id: uuid::Uuid::new_v4().to_string(),
        name: path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "média".into()),
        path: path.to_string_lossy().into_owned(),
        kind,
        duration,
        width: video.and_then(|s| s.width).unwrap_or(0),
        height: video.and_then(|s| s.height).unwrap_or(0),
        fps,
        video_codec: video.and_then(|s| s.codec_name.clone()),
        audio_codec: audio.and_then(|s| s.codec_name.clone()),
        audio_channels: audio.and_then(|s| s.channels).unwrap_or(0),
        audio_sample_rate: audio
            .and_then(|s| s.sample_rate.as_deref())
            .and_then(|s| s.trim().parse::<u32>().ok())
            .unwrap_or(0),
        size_bytes,
        thumbnail_path: None,
        preview_path: None,
        waveform: None,
        browser_playable: browser_playable(&ext, video.and_then(|s| s.codec_name.as_deref())),
    })
}

/// Can a Chromium-family browser decode this file as-is?
///
/// The container is the decisive half: Chrome refuses `video/x-matroska` even
/// when the codec inside (H.264) would otherwise be fine. HEVC is only reliable
/// on platforms with hardware support, so it is treated as not playable and
/// served through a proxy — which also covers the 10-bit and 4:2:2 variants that
/// browsers reject outright.
fn browser_playable(ext: &str, codec: Option<&str>) -> bool {
    let codec = codec.unwrap_or("");
    match ext {
        "mp4" | "m4v" | "mov" => matches!(codec, "h264" | "av1"),
        "webm" => matches!(codec, "vp8" | "vp9" | "av1"),
        _ => false,
    }
}

#[cfg(test)]
mod playable_tests {
    use super::browser_playable;

    #[test]
    fn containers_decide_as_much_as_codecs() {
        assert!(browser_playable("mp4", Some("h264")));
        assert!(browser_playable("webm", Some("vp9")));
        // Matroska is refused by browsers even with a playable codec inside.
        assert!(!browser_playable("mkv", Some("h264")));
        // HEVC needs hardware support the browser cannot be assumed to have.
        assert!(!browser_playable("mp4", Some("hevc")));
        assert!(!browser_playable("avi", Some("mpeg4")));
    }
}

/// An MP3 with an embedded cover, or any file whose only video streams are
/// still images, is audio as far as editing is concerned.
fn is_cover_art_only(parsed: &ProbeOutput) -> bool {
    let videos: Vec<&ProbeStream> = parsed
        .streams
        .iter()
        .filter(|s| s.codec_type.as_deref() == Some("video"))
        .collect();
    !videos.is_empty()
        && videos.iter().all(|s| {
            matches!(
                s.codec_name.as_deref(),
                Some("mjpeg") | Some("png") | Some("bmp")
            ) && s.avg_frame_rate.as_deref() == Some("0/0")
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rationals_parse() {
        assert!((parse_rational("30000/1001").unwrap() - 29.97).abs() < 0.01);
        assert_eq!(parse_rational("25/1"), Some(25.0));
        assert_eq!(parse_rational("0/0"), None);
        assert_eq!(parse_rational("garbage"), None);
    }
}
