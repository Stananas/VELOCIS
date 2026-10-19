//! Running a `RenderPlan` through ffmpeg, with progress reporting, hardware
//! encoder selection and cancellation.
//!
//! Encoder choice is *probed*, not assumed: a broken or unavailable VAAPI device
//! is discovered by actually attempting a one-frame encode, and the engine then
//! falls back to libx264 rather than failing the user's export at 90%.

use crate::model::{Micros, ONE_SECOND};
use crate::probe::ffmpeg_bin;
use crate::render::{RenderPlan, RenderTarget};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Container {
    Mp4,
    /// Serde's snake_case would render this as `web_m`, which is not a name any
    /// client would guess, so it is spelled out.
    #[serde(rename = "webm")]
    WebM,
}

impl Container {
    pub fn extension(self) -> &'static str {
        match self {
            Container::Mp4 => "mp4",
            Container::WebM => "webm",
        }
    }
}

/// Quality is expressed as a 0..1 slider; each encoder maps it to its own scale.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Quality {
    Draft,
    Standard,
    High,
    Best,
}

impl Quality {
    /// libx264 CRF: lower is better.
    fn crf(self) -> u32 {
        match self {
            Quality::Draft => 30,
            Quality::Standard => 26,
            Quality::High => 22,
            Quality::Best => 18,
        }
    }
    /// Constant-QP for hardware encoders.
    fn qp(self) -> u32 {
        match self {
            Quality::Draft => 32,
            Quality::Standard => 27,
            Quality::High => 23,
            Quality::Best => 19,
        }
    }
    fn audio_kbps(self) -> u32 {
        match self {
            Quality::Draft => 96,
            Quality::Standard => 128,
            Quality::High => 192,
            Quality::Best => 256,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ExportSettings {
    pub output: PathBuf,
    pub target: RenderTarget,
    pub container: Container,
    pub quality: Quality,
    /// The encoder that will run. Owned by the settings rather than re-derived
    /// inside `run`, so the filter graph and the codec can never disagree: a
    /// VAAPI run needs `hwupload` in the graph, and a mismatch is an
    /// unactionable ffmpeg error for the user.
    pub encoder: Encoder,
}

impl ExportSettings {
    /// Build settings, choosing the encoder for this exact output size.
    pub fn new(
        output: PathBuf,
        width: u32,
        height: u32,
        fps: f64,
        container: Container,
        quality: Quality,
    ) -> Self {
        let encoder = select_encoder(container, width, height);
        let mut target = RenderTarget::new(width, height, fps);
        if encoder == Encoder::Vaapi {
            // Hardware frames must be uploaded inside the graph: ffmpeg refuses
            // to combine `-vf` with a stream fed by `-filter_complex`.
            target.finish = Some("format=nv12,hwupload".to_string());
        }
        Self {
            output,
            target,
            container,
            quality,
            encoder,
        }
    }
}

/// Choose the best encoder for a given container and output size.
fn select_encoder(container: Container, width: u32, height: u32) -> Encoder {
    match container {
        // VP9 hardware support is far less predictable than H.264, so a WebM
        // export never silently produces a broken file.
        Container::WebM => Encoder::Software,
        Container::Mp4 => {
            if std::env::var("VELOCIS_FORCE_CPU_ENCODER").is_ok() {
                return Encoder::Software;
            }
            // VAAPI's H.264 encoder refuses anything below 128x128.
            if width < 128 || height < 128 {
                return Encoder::Software;
            }
            if probe_encoder() == Encoder::Software && probe_nvenc() {
                Encoder::Nvenc
            } else {
                probe_encoder()
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Encoder {
    /// AMD/Intel VAAPI through `/dev/dri`.
    Vaapi,
    /// Nvidia NVENC.
    Nvenc,
    /// libx264 / libvpx-vp9 on the CPU.
    Software,
}

impl Encoder {
    pub fn label(self) -> &'static str {
        match self {
            Encoder::Vaapi => "VAAPI (GPU)",
            Encoder::Nvenc => "NVENC (GPU)",
            Encoder::Software => "CPU",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ExportProgress {
    /// 0.0..=1.0
    pub ratio: f64,
    pub out_time: Micros,
    /// Encoding rate relative to real time, as reported by ffmpeg.
    pub speed: f64,
    pub frame: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExportReport {
    pub output: String,
    pub encoder: Encoder,
    pub duration: Micros,
    pub bytes: u64,
    pub elapsed_register: f64,
}

/// A one-frame probe, cached for the process lifetime.
fn probe_encoder() -> Encoder {
    static CHOSEN: OnceLock<Encoder> = OnceLock::new();
    *CHOSEN.get_or_init(|| {
        if std::env::var("VELOCIS_FORCE_CPU_ENCODER").is_ok() {
            return Encoder::Software;
        }
        let devices: Vec<String> = ["/dev/dri/renderD128", "/dev/dri/renderD129"]
            .iter()
            .filter(|d| Path::new(d).exists())
            .map(|d| d.to_string())
            .collect();
        for device in devices {
            if probe_vaapi(&device) {
                return Encoder::Vaapi;
            }
        }
        Encoder::Software
    })
}

fn probe_vaapi(device: &str) -> bool {
    // 320x240 sits comfortably inside VAAPI's 128..4096 constraint: probing at a
    // smaller size reports "hardware does not support encoding at size" and
    // would silently disable a perfectly good GPU.
    let result = Command::new(ffmpeg_bin())
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-vaapi_device",
            device,
        ])
        .args(["-f", "lavfi", "-i", "color=c=black:s=320x240:r=30:d=0.1"])
        .args(["-vf", "format=nv12,hwupload"])
        .args(["-c:v", "h264_vaapi", "-frames:v", "1", "-f", "null", "-"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    matches!(result, Ok(s) if s.success())
}

fn probe_nvenc() -> bool {
    let result = Command::new(ffmpeg_bin())
        .args(["-hide_banner", "-loglevel", "error"])
        .args(["-f", "lavfi", "-i", "color=c=black:s=128x64:r=30:d=0.1"])
        .args(["-c:v", "h264_nvenc", "-frames:v", "1", "-f", "null", "-"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    matches!(result, Ok(s) if s.success())
}

/// The encoder this build will actually use.
pub fn selected_encoder() -> Encoder {
    probe_encoder()
}

/// Everything a caller needs to know before starting an export.
#[derive(Debug, Clone, Serialize)]
pub struct ExportCapabilities {
    pub encoder: Encoder,
    pub hardware_accelerated: bool,
    pub containers: Vec<&'static str>,
}

pub fn capabilities() -> ExportCapabilities {
    let encoder = selected_encoder();
    ExportCapabilities {
        encoder,
        hardware_accelerated: encoder != Encoder::Software,
        containers: vec!["mp4", "webm"],
    }
}

/// Run the export. `progress` returns `false` to cancel; the child is then
/// killed and the partial file removed.
pub fn run(
    plan: &RenderPlan,
    settings: &ExportSettings,
    progress: &mut dyn FnMut(ExportProgress) -> bool,
) -> Result<ExportReport> {
    // Text payloads are referenced by path from the graph; materialise them now.
    for file in &plan.text_files {
        if let Some(parent) = file.path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("création de {} impossible", parent.display()))?;
        }
        let current = std::fs::read_to_string(&file.path).ok();
        if current.as_deref() != Some(file.content.as_str()) {
            std::fs::write(&file.path, &file.content)
                .with_context(|| format!("écriture de {} impossible", file.path.display()))?;
        }
    }

    let encoder = settings.encoder;
    // Fail loudly here rather than letting ffmpeg produce a cryptic error.
    if encoder == Encoder::Vaapi && settings.target.finish.is_none() {
        bail!(
            "incohérence interne : encodeur VAAPI sans étape hwupload dans le graphe \
             (utilisez ExportSettings::new pour construire les réglages)"
        );
    }

    let mut cmd = Command::new(ffmpeg_bin());
    cmd.args(["-hide_banner", "-loglevel", "error", "-y"]);
    cmd.args(["-progress", "pipe:1", "-nostats"]);

    if encoder == Encoder::Vaapi {
        // Must be set before the inputs so `hwupload` inside the graph has a
        // device to target.
        cmd.args(["-vaapi_device", "/dev/dri/renderD128"]);
    }

    for input in &plan.inputs {
        cmd.arg("-i").arg(input);
    }

    cmd.arg("-filter_complex").arg(&plan.filter_complex);

    if let Some(v) = &plan.video_label {
        cmd.arg("-map").arg(v);
    }
    if let Some(a) = &plan.audio_label {
        cmd.arg("-map").arg(a);
    }

    match (settings.container, encoder) {
        (Container::Mp4, Encoder::Vaapi) => {
            cmd.args(["-c:v", "h264_vaapi"])
                .args(["-rc_mode", "CQP"])
                .args(["-qp", &settings.quality.qp().to_string()]);
        }
        (Container::Mp4, Encoder::Nvenc) => {
            cmd.args(["-c:v", "h264_nvenc"])
                .args(["-preset", "p5"])
                .args(["-rc", "constqp"])
                .args(["-qp", &settings.quality.qp().to_string()]);
        }
        (Container::Mp4, Encoder::Software) => {
            cmd.args(["-c:v", "libx264"])
                .args(["-preset", "medium"])
                .args(["-crf", &settings.quality.crf().to_string()]);
        }
        (Container::WebM, _) => {
            cmd.args(["-c:v", "libvpx-vp9"])
                .args(["-crf", &settings.quality.crf().to_string()])
                .args(["-b:v", "0"])
                .args(["-row-mt", "1"])
                .args(["-deadline", "good"])
                .args(["-cpu-used", "3"]);
        }
    }

    match settings.container {
        Container::Mp4 => {
            cmd.args(["-c:a", "aac"])
                .args(["-b:a", &format!("{}k", settings.quality.audio_kbps())])
                .args(["-movflags", "+faststart"]);
            // Hardware encoders own their pixel format; asking for yuv420p on a
            // VAAPI stream is rejected.
            if encoder == Encoder::Software {
                cmd.args(["-pix_fmt", "yuv420p"]);
            }
        }
        Container::WebM => {
            cmd.args(["-c:a", "libopus"])
                .args(["-b:a", &format!("{}k", settings.quality.audio_kbps())]);
        }
    }

    // Bound the output: a stray infinite source must not produce an endless file.
    cmd.args([
        "-t",
        &format!("{:.6}", plan.duration as f64 / ONE_SECOND as f64),
    ]);
    cmd.args(["-r", &format!("{}", settings.target.fps)]);
    if let Some(parent) = settings.output.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    cmd.arg(&settings.output);
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let started = std::time::Instant::now();
    let mut child = cmd
        .spawn()
        .with_context(|| format!("lancement de {} impossible", ffmpeg_bin()))?;

    let stdout = child.stdout.take().context("stdout ffmpeg indisponible")?;
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    let mut cancelled = false;
    let total = plan.duration.max(1) as f64;

    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {}
            Err(_) => break,
        }
        let (key, value) = match line.trim().split_once('=') {
            Some(kv) => kv,
            None => continue,
        };
        match key {
            "out_time_us" => {
                let out_time: Micros = value.trim().parse().unwrap_or(0);
                let ratio = (out_time as f64 / total).clamp(0.0, 1.0);
                if !progress(ExportProgress {
                    ratio,
                    out_time,
                    speed: 0.0,
                    frame: 0,
                }) {
                    cancelled = true;
                    break;
                }
            }
            "frame" => { /* reported through out_time_us; kept for future use */ }
            "progress" if value.trim() == "end" => break,
            _ => {}
        }
    }

    if cancelled {
        let _ = child.kill();
        let _ = child.wait();
        let _ = std::fs::remove_file(&settings.output);
        bail!("export annulé");
    }

    let output = child.wait_with_output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("ffmpeg a échoué: {}", tail(&stderr, 2000));
    }

    let bytes = std::fs::metadata(&settings.output)
        .map(|m| m.len())
        .unwrap_or(0);
    if bytes == 0 {
        bail!("ffmpeg s'est terminé sans écrire de fichier");
    }

    progress(ExportProgress {
        ratio: 1.0,
        out_time: plan.duration,
        speed: 0.0,
        frame: 0,
    });

    Ok(ExportReport {
        output: settings.output.to_string_lossy().into_owned(),
        encoder,
        duration: plan.duration,
        bytes,
        elapsed_register: started.elapsed().as_secs_f64(),
    })
}

/// Keep the informative end of an ffmpeg error message.
fn tail(text: &str, max: usize) -> String {
    let text = text.trim();
    if text.len() <= max {
        return text.to_string();
    }
    let cut = text.len() - max;
    // Do not slice mid-UTF-8.
    let start = text
        .char_indices()
        .find(|(i, _)| *i >= cut)
        .map(|(i, _)| i)
        .unwrap_or(0);
    format!("…{}", &text[start..])
}

/// Suggested file name for a render of this project.
pub fn suggested_output(project: &crate::model::Project, container: Container) -> String {
    let slug: String = project
        .name
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_ascii_lowercase();
    let slug = if slug.is_empty() {
        "clip".to_string()
    } else {
        slug
    };
    format!("{slug}.{}", container.extension())
}

/// Guarantee that a caller-supplied name carries the container's extension.
///
/// ffmpeg picks its muxer from the extension and refuses to guess, so a missing
/// one is a hard failure. Comparing suffixes is not enough: `test-webm` ends with
/// `webm` while having no extension at all.
pub fn with_container_extension(name: &str, container: Container) -> String {
    let trimmed = name.trim();
    let wanted = container.extension();
    let already = std::path::Path::new(trimmed)
        .extension()
        .map(|ext| ext.eq_ignore_ascii_case(wanted))
        .unwrap_or(false);
    if already {
        trimmed.to_string()
    } else {
        format!("{}.{}", trimmed.trim_end_matches('.'), wanted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn container_names_are_what_a_client_would_guess() {
        // Serde's snake_case turns `WebM` into `web_m`; the wire format must not
        // depend on that accident.
        assert_eq!(serde_json::to_string(&Container::Mp4).unwrap(), "\"mp4\"");
        assert_eq!(serde_json::to_string(&Container::WebM).unwrap(), "\"webm\"");
        assert_eq!(
            serde_json::from_str::<Container>("\"webm\"").unwrap(),
            Container::WebM
        );
    }

    #[test]
    fn output_names_always_end_with_the_container_extension() {
        let cases = [
            ("abc", "abc.webm"),
            // A suffix that merely *looks* like the extension must not count.
            ("test-webm", "test-webm.webm"),
            ("test-webm.webm", "test-webm.webm"),
            ("test.WEBM", "test.WEBM"),
            ("abc.mp4", "abc.mp4.webm"),
            ("fin.", "fin.webm"),
        ];
        for (input, expected) in cases {
            assert_eq!(
                with_container_extension(input, Container::WebM),
                expected,
                "entrée: {input}"
            );
        }
        assert_eq!(with_container_extension("film", Container::Mp4), "film.mp4");
    }

    #[test]
    fn quality_scales_are_monotonic() {
        let order = [
            Quality::Draft,
            Quality::Standard,
            Quality::High,
            Quality::Best,
        ];
        for pair in order.windows(2) {
            assert!(pair[0].crf() > pair[1].crf(), "CRF should decrease");
            assert!(pair[0].qp() > pair[1].qp(), "QP should decrease");
            assert!(pair[0].audio_kbps() < pair[1].audio_kbps());
        }
    }

    #[test]
    fn tails_are_utf8_safe() {
        let text = "é".repeat(100);
        let out = tail(&text, 10);
        assert!(out.starts_with('…'));
        assert!(out.len() > 1);
    }

    #[test]
    fn suggested_names_are_slugged() {
        let mut project = crate::model::Project::new(
            "Mon Super Projet !",
            crate::model::Canvas {
                width: 1920,
                height: 1080,
                fps: 30.0,
                background: crate::model::Rgba::BLACK,
            },
        );
        project.name = "Mon Super Projet !".into();
        assert_eq!(
            suggested_output(&project, Container::Mp4),
            "mon-super-projet.mp4"
        );
    }
}
