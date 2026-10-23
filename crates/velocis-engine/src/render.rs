//! `Project` -> ffmpeg filter graph.
//!
//! The render step is expressed as a *value* (`RenderPlan`) rather than as a
//! sequence of imperative steps. That buys three things:
//!
//! 1. the same plan serves both the low-resolution preview and the full-quality
//!    export, so what you see is what you get;
//! 2. it can be inspected, logged and unit-tested without running ffmpeg;
//! 3. a project file alone is enough to reproduce a render.
//!
//! # Time bases
//!
//! Three clocks appear in the graph and mixing them up is the classic source of
//! "the clip is one second late" bugs, so they are named explicitly:
//!
//! * **source time** — position inside the asset file;
//! * **clip time** — 0 at the clip's first frame on the timeline;
//! * **output time** — 0 at the start of the rendered window.
//!
//! A clip's fades belong to *clip time*, so they are applied while the stream is
//! still in that base and can never go negative — `fade` rejects a negative
//! start time. The stream is then rebased to output time and padded with
//! transparent frames so its overlay input is defined across the whole window.
//!
//! # Composition
//!
//! A transparent canvas is the accumulator. Each visual clip is prepared in
//! isolation (trim -> geometry -> look -> opacity -> fades -> speed), then
//! composited with `overlay` gated by `enable=between(t,start,end)`. Audio
//! mirrors it: `atrim`/`asetpts`/`atempo`/`volume`/`afade`, delayed to its
//! position with `adelay`, then summed with `amix`.
//!
//! Synthetic sources (`color`) are used in-graph rather than as extra inputs, so
//! text clips, the canvas background and the silent-audio fallback all go
//! through exactly the same transform/fade/opacity code path as real footage.

use crate::model::*;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

const DEFAULT_FONT: &str = "DejaVu Sans";

/// Deduplicated list of file inputs, in the order ffmpeg must receive them.
#[derive(Default)]
struct InputSet {
    list: Vec<PathBuf>,
    index: HashMap<String, usize>,
}

impl InputSet {
    fn file(&mut self, path: &str) -> usize {
        if let Some(i) = self.index.get(path) {
            return *i;
        }
        let i = self.list.len();
        self.list.push(PathBuf::from(path));
        self.index.insert(path.to_string(), i);
        i
    }
}

/// A text payload the executor must write to disk before running ffmpeg.
///
/// User text is kept out of the filter graph entirely: `drawtext` reads it from
/// a file, so no amount of `:`, `,`, `'`, `[`, `%` or newline in a title can
/// break the graph. The path is derived from a content hash, which keeps the
/// plan a pure value even though it references files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextFile {
    pub path: PathBuf,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct RenderPlan {
    pub inputs: Vec<PathBuf>,
    pub filter_complex: String,
    pub video_label: Option<String>,
    pub audio_label: Option<String>,
    /// Output duration, in microseconds.
    pub duration: Micros,
    pub text_files: Vec<TextFile>,
    pub warnings: Vec<String>,
}

impl RenderPlan {
    /// Human-readable form of the graph, for logs and bug reports.
    ///
    /// The graph itself must never contain newlines, so pretty-printing is a
    /// presentation concern kept strictly separate from the value ffmpeg gets.
    pub fn pretty(&self) -> String {
        let mut out = String::with_capacity(self.filter_complex.len() + 64);
        for (i, chain) in self.filter_complex.split(';').enumerate() {
            if chain.is_empty() {
                continue;
            }
            out.push_str(&format!("{i:>3}: {chain}\n"));
        }
        out
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RenderRange {
    pub from: Micros,
    pub to: Micros,
}

impl RenderRange {
    pub fn whole(project: &Project) -> Self {
        Self {
            from: 0,
            to: project.duration().max(ONE_SECOND),
        }
    }
    pub fn duration(&self) -> Micros {
        (self.to - self.from).max(0)
    }
}

/// Output properties of a render, plus an optional filter-chain suffix.
///
/// `finish` exists so hardware upload can live *inside* the graph. ffmpeg
/// refuses to combine `-vf` with a stream fed by `-filter_complex`, so the
/// `format=nv12,hwupload` step has to be part of the plan rather than a
/// command-line flag.
#[derive(Debug, Clone)]
pub struct RenderTarget {
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub finish: Option<String>,
    /// Where text payload files are written. Content-addressed, so the plan stays
    /// deterministic for a given target.
    pub text_dir: PathBuf,
}

impl RenderTarget {
    pub fn new(width: u32, height: u32, fps: f64) -> Self {
        Self {
            width,
            height,
            fps,
            finish: None,
            text_dir: default_text_dir(),
        }
    }
}

/// Text rasters live beside the rest of the application's data, never in the
/// system temporary directory.
fn default_text_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("VELOCIS_TEXT_DIR") {
        return PathBuf::from(dir);
    }
    let base = std::env::var("VELOCIS_DATA_DIR")
        .map(PathBuf::from)
        .ok()
        .filter(|p| p.is_absolute())
        .or_else(|| {
            std::env::var("XDG_DATA_HOME")
                .map(PathBuf::from)
                .ok()
                .filter(|p| p.is_absolute())
        })
        .or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(|home| PathBuf::from(home).join(".local").join("share"))
        })
        .unwrap_or_else(std::env::temp_dir);
    base.join("velocis").join("text")
}

/// One clip resolved against the requested render range.
struct ResolvedClip<'a> {
    clip: &'a Clip,
    asset: Option<&'a MediaAsset>,
    /// Position on the *output* timeline.
    start: Micros,
    /// Visible length inside the render window.
    visible: Micros,
    /// The clip's own full length on the timeline, independent of the window.
    clip_duration: Micros,
    /// How far into the clip the window begins (0 when the window covers its
    /// first frame).
    into: Micros,
    /// Offset into the clip's own source material.
    source_start: Micros,
    /// Vertical order: 0 is the bottom-most track.
    z: usize,
}

fn is_visual(asset: Option<&MediaAsset>, clip: &Clip) -> bool {
    clip.is_text() || asset.map(|a| a.kind != AssetKind::Audio).unwrap_or(false)
}

fn has_audio(asset: Option<&MediaAsset>) -> bool {
    asset.map(|a| a.audio_codec.is_some()).unwrap_or(false)
}

/// A fade is capped at half the clip: two longer fades would cross and the clip
/// would never reach full opacity.
fn fade_duration(fade: Micros, clip_duration: Micros) -> Micros {
    fade.min(clip_duration / 2).max(0)
}

/// Build the plan for the default (software) target.
pub fn build_plan(
    project: &Project,
    range: &RenderRange,
    width: u32,
    height: u32,
    fps: f64,
) -> RenderPlan {
    build_plan_with(project, range, &RenderTarget::new(width, height, fps))
}

/// Build the plan. `target` overrides the canvas: that is how the preview
/// renders cheaper than the export while sharing one code path.
pub fn build_plan_with(
    project: &Project,
    range: &RenderRange,
    target: &RenderTarget,
) -> RenderPlan {
    let (width, height, fps) = (target.width, target.height, target.fps);
    let mut warnings = Vec::new();
    let mut inputs = InputSet::default();
    let mut graph = String::new();
    let mut text_files: Vec<TextFile> = Vec::new();
    let duration = range.duration().max(1);

    // ---- resolve clips against the requested window ---------------------
    let mut resolved: Vec<ResolvedClip> = Vec::new();
    for (z, track) in project.tracks.iter().enumerate() {
        if track.hidden {
            continue;
        }
        for clip in &track.clips {
            let asset = project.asset(&clip.asset_id);
            if asset.is_none() && !clip.is_text() {
                warnings.push(format!("clip {} ignoré : média introuvable", clip.id));
                continue;
            }
            let start = clip.timeline.start.max(range.from);
            let end = clip.timeline.end().min(range.to);
            if end <= start {
                continue;
            }
            let speed = clip.speed.clamp(0.1, 20.0);
            let into = start - clip.timeline.start;
            let is_still = asset.map(|a| a.kind) == Some(AssetKind::Image);
            // Synthetic and still sources are indexed in clip time; real footage
            // advances at `speed`, so the source offset scales with it.
            let source_start = if clip.is_text() || is_still {
                into
            } else {
                clip.source.start + ((into as f64) * speed).round() as Micros
            };
            resolved.push(ResolvedClip {
                clip,
                asset,
                start: start - range.from,
                visible: end - start,
                clip_duration: clip.timeline.duration,
                into,
                source_start,
                z,
            });
        }
    }

    // Bottom track first, then left to right, then by id: a deterministic order
    // means the same project always produces the same graph.
    resolved.sort_by(|a, b| {
        a.z.cmp(&b.z)
            .then(a.start.cmp(&b.start))
            .then(a.clip.id.cmp(&b.clip.id))
    });

    // ---- accumulator ----------------------------------------------------
    // Chains are separated by `;`, never by a newline: ffmpeg's graph parser
    // rejects a multi-line `-filter_complex` argument, and the failure mode is
    // an opaque "Trailing garbage after a filter".
    write!(
        graph,
        "color=c={}@{}:s={width}x{height}:r={}:d={}[base];",
        hex(&project.canvas.background),
        alpha(&project.canvas.background),
        frame_rate(fps),
        seconds(duration)
    )
    .ok();
    let mut current = "[base]".to_string();
    let mut step = 0usize;

    // ---- video ----------------------------------------------------------
    for item in &resolved {
        if !is_visual(item.asset, item.clip) {
            continue;
        }
        let speed = item.clip.speed.clamp(0.1, 20.0);
        let is_synthetic =
            item.clip.is_text() || item.asset.map(|a| a.kind) == Some(AssetKind::Image);

        let mut chain = match &item.clip.text {
            Some(text) => {
                let content = escape_drawtext_expansions(&text.content);
                let path = target
                    .text_dir
                    .join(format!("{:016x}.txt", fnv1a(content.as_bytes())));
                if !text_files.iter().any(|t| t.path == path) {
                    text_files.push(TextFile {
                        path: path.clone(),
                        content,
                    });
                }
                format!(
                    "color=c=black@0:s={width}x{height}:r={}:d={},format=rgba,{}",
                    frame_rate(fps),
                    seconds(item.clip_duration),
                    drawtext_chain(text, &path)
                )
            }
            None => {
                let asset = item.asset.expect("vérifié plus haut");
                let index = inputs.file(&asset.path);
                let mut c = format!("[{index}:v]");
                if asset.kind == AssetKind::Image {
                    // A still has to be repeated for the clip's whole length.
                    write!(
                        c,
                        "loop=loop=-1:size=1:start=0,trim=duration={},",
                        seconds(item.clip_duration)
                    )
                    .ok();
                }
                c
            }
        };

        // A synthetic source is consumed one-for-one with clip time; real
        // footage is consumed `speed` times faster.
        let consume = if is_synthetic {
            item.visible
        } else {
            ((item.visible as f64) * speed).round() as Micros
        };
        write!(
            chain,
            "trim=start={}:duration={},",
            seconds(item.source_start),
            seconds(consume)
        )
        .ok();
        if (speed - 1.0).abs() > 1e-9 {
            write!(chain, "setpts=(PTS-STARTPTS)/{speed:.6},").ok();
        } else {
            chain.push_str("setpts=PTS-STARTPTS,");
        }
        // Enter clip time so fades can be expressed with non-negative offsets.
        if item.into > 0 {
            write!(chain, "setpts=PTS+{}/TB,", seconds(item.into)).ok();
        }

        // --- geometry ----------------------------------------------------
        let t = item.clip.transform;
        let cropped =
            t.crop_left != 0.0 || t.crop_right != 0.0 || t.crop_top != 0.0 || t.crop_bottom != 0.0;
        // Contain inside the canvas first, so `scale` means the same thing for
        // every source resolution.
        write!(
            chain,
            "scale=w={width}:h={height}:force_original_aspect_ratio=decrease:eval=init,"
        )
        .ok();
        // Crop *after* the fit, so trimming an edge hides it instead of zooming:
        // dragging a crop handle must not move the picture.
        if cropped {
            let w = (1.0 - t.crop_left - t.crop_right).clamp(0.01, 1.0);
            let h = (1.0 - t.crop_top - t.crop_bottom).clamp(0.01, 1.0);
            write!(
                chain,
                "crop=w=iw*{w:.6}:h=ih*{h:.6}:x=iw*{:.6}:y=ih*{:.6},",
                t.crop_left.clamp(0.0, 0.99),
                t.crop_top.clamp(0.0, 0.99)
            )
            .ok();
        }
        let scale = t.scale.clamp(0.01, 20.0);
        if (scale - 1.0).abs() > 1e-9 {
            write!(chain, "scale=w=iw*{scale:.6}:h=ih*{scale:.6}:eval=init,").ok();
        }
        if t.mirror_x {
            chain.push_str("hflip,");
        }
        if t.mirror_y {
            chain.push_str("vflip,");
        }
        if (t.rotation_deg % 360.0).abs() > 1e-9 {
            write!(
                chain,
                "rotate=a={:.6}*PI/180:c=black@0:ow=rotw(iw):oh=roth(ih),",
                t.rotation_deg
            )
            .ok();
        }

        // --- look --------------------------------------------------------
        chain.push_str(&effects_chain(&item.clip.effects, &mut warnings));

        // --- opacity and fades -------------------------------------------
        chain.push_str("format=rgba,");
        let opacity = item.clip.opacity.clamp(0.0, 1.0);
        if opacity < 1.0 {
            write!(chain, "colorchannelmixer=aa={opacity:.6},").ok();
        }
        let fade_in = fade_duration(item.clip.fade_in, item.clip_duration);
        if fade_in > 0 {
            write!(
                chain,
                "fade=t=in:st={}:d={}:alpha=1,",
                seconds(0),
                seconds(fade_in)
            )
            .ok();
        }
        let fade_out = fade_duration(item.clip.fade_out, item.clip_duration);
        if fade_out > 0 {
            write!(
                chain,
                "fade=t=out:st={}:d={}:alpha=1,",
                seconds(item.clip_duration - fade_out),
                seconds(fade_out)
            )
            .ok();
        }

        // --- back to output time, then place ----------------------------
        if item.into > 0 {
            write!(chain, "setpts=PTS-{}/TB,", seconds(item.into)).ok();
        }
        let start = item.start.max(0);
        // `tpad` materialises transparent frames before the clip so the overlay
        // stream is defined across the whole window. Timestamps are left alone
        // afterwards: rebasing them would slide the clip in time and break the
        // `enable` gate below.
        write!(
            chain,
            "format=rgba,tpad=start_duration={}:start_mode=add:color=black@0,",
            seconds(start)
        )
        .ok();
        write!(
            chain,
            "trim=end={}[ov{step}]",
            seconds(start + item.visible)
        )
        .ok();
        write!(graph, "{chain};").ok();

        let out = format!("[v{}]", step + 1);
        write!(
            graph,
            "{current}[ov{step}]overlay=x=(W-w)/2+{:.2}:y=(H-h)/2+{:.2}:eval=init:eof_action=pass:repeatlast=0:enable='between(t,{},{} )'{out};",
            t.x,
            t.y,
            seconds(start),
            seconds(start + item.visible)
        )
        .ok();
        current = out;
        step += 1;
    }

    if step == 0 {
        warnings.push("aucun clip visible : rendu d'un fond uni".into());
    }
    if let Some(finish) = &target.finish {
        write!(graph, "{current}{finish}[vout];").ok();
    } else {
        write!(graph, "{current}format=yuv420p[vout];").ok();
    }

    // ---- audio ----------------------------------------------------------
    let mut audio_labels: Vec<String> = Vec::new();
    let mut audio_step = 0usize;
    for item in &resolved {
        let asset = match item.asset {
            Some(a) if has_audio(Some(a)) => a,
            _ => continue,
        };
        let index = inputs.file(&asset.path);
        let speed = item.clip.speed.clamp(0.1, 20.0);
        let consume = ((item.visible as f64) * speed).round() as Micros;

        let mut chain = format!("[{index}:a]");
        write!(
            chain,
            "atrim=start={}:duration={},asetpts=PTS-STARTPTS,",
            seconds(item.source_start),
            seconds(consume)
        )
        .ok();
        if (speed - 1.0).abs() > 1e-9 {
            chain.push_str(&atempo_chain(speed));
        }
        let volume = item.clip.volume.clamp(0.0, 8.0);
        if (volume - 1.0).abs() > 1e-9 {
            write!(chain, "volume={volume:.4},").ok();
        }
        if item.into > 0 {
            write!(chain, "asetpts=PTS+{}/TB,", seconds(item.into)).ok();
        }
        let fade_in = fade_duration(item.clip.fade_in, item.clip_duration);
        if fade_in > 0 {
            write!(
                chain,
                "afade=t=in:st={}:d={},",
                seconds(0),
                seconds(fade_in)
            )
            .ok();
        }
        let fade_out = fade_duration(item.clip.fade_out, item.clip_duration);
        if fade_out > 0 {
            write!(
                chain,
                "afade=t=out:st={}:d={},",
                seconds(item.clip_duration - fade_out),
                seconds(fade_out)
            )
            .ok();
        }
        if item.into > 0 {
            write!(chain, "asetpts=PTS-{}/TB,", seconds(item.into)).ok();
        }
        // `all=1` delays every channel by the same amount.
        write!(
            chain,
            "adelay={}:all=1[ap{audio_step}]",
            (item.start.max(0) / 1000).max(0)
        )
        .ok();
        write!(graph, "{chain};").ok();
        audio_labels.push(format!("[ap{audio_step}]"));
        audio_step += 1;
    }

    let audio_label = match audio_labels.len() {
        0 => {
            // A silent track is friendlier downstream than no track at all.
            write!(
                graph,
                "anullsrc=channel_layout=stereo:sample_rate=48000,atrim=duration={},asetpts=PTS-STARTPTS[aout];",
                seconds(duration)
            )
            .ok();
            "[aout]".to_string()
        }
        1 => {
            write!(graph, "{}anull[aout];", audio_labels[0]).ok();
            "[aout]".to_string()
        }
        n => {
            // Mixing many sources clips easily, so a limiter protects the sum.
            write!(
                graph,
                "{}amix=inputs={n}:normalize=0:dropout_transition=0,alimiter=limit=0.97[aout];",
                audio_labels.join("")
            )
            .ok();
            "[aout]".to_string()
        }
    };

    RenderPlan {
        inputs: inputs.list,
        filter_complex: graph,
        video_label: Some("[vout]".into()),
        audio_label: Some(audio_label),
        duration,
        text_files,
        warnings,
    }
}

/// ffmpeg's `atempo` only accepts 0.5..=100, so extreme speeds are chained.
fn atempo_chain(speed: f64) -> String {
    let mut out = String::new();
    let mut remaining = speed;
    while remaining > 100.0 {
        out.push_str("atempo=100,");
        remaining /= 100.0;
    }
    while remaining < 0.5 {
        out.push_str("atempo=0.5,");
        remaining /= 0.5;
    }
    if (remaining - 1.0).abs() > 1e-9 {
        write!(out, "atempo={remaining:.6},").ok();
    }
    out
}

/// Map the declarative effect list onto ffmpeg filters, in order.
fn effects_chain(effects: &[Effect], warnings: &mut Vec<String>) -> String {
    let mut out = String::new();
    for effect in effects {
        match effect {
            Effect::Brightness { amount } => {
                write!(out, "eq=brightness={:.4},", amount.clamp(-1.0, 1.0)).ok();
            }
            Effect::Contrast { amount } => {
                write!(out, "eq=contrast={:.4},", (1.0 + amount).clamp(0.0, 4.0)).ok();
            }
            Effect::Saturation { amount } => {
                write!(out, "eq=saturation={:.4},", (1.0 + amount).clamp(0.0, 4.0)).ok();
            }
            Effect::Gamma { amount } => {
                write!(out, "eq=gamma={:.4},", amount.clamp(0.1, 10.0)).ok();
            }
            Effect::Hue { degrees } => {
                write!(out, "hue=h={degrees:.4},").ok();
            }
            Effect::Temperature { kelvin } => {
                write!(
                    out,
                    "colortemperature=temperature={:.0},",
                    kelvin.clamp(1000.0, 40000.0)
                )
                .ok();
            }
            Effect::Blur { amount } => {
                let radius = (amount.clamp(0.0, 1.0) * 20.0).max(0.5);
                write!(out, "boxblur=luma_radius={radius:.2}:luma_power=1,").ok();
            }
            Effect::Sharpen { amount } => {
                write!(out, "unsharp=5:5:{:.3}:5:5:0,", amount.clamp(0.0, 2.0)).ok();
            }
            Effect::Grayscale { amount } => {
                // Removing all saturation is exactly a luminance-only image.
                write!(out, "hue=s={:.4},", (1.0 - amount).clamp(0.0, 1.0)).ok();
            }
            Effect::Sepia { amount } => {
                // Interpolate between the identity and the classic sepia matrix
                // so the slider stays continuous.
                let a = amount.clamp(0.0, 1.0);
                let lerp = |v: f64| 1.0 + a * (v - 1.0);
                write!(
                    out,
                    "colorchannelmixer=rr={:.4}:rg={:.4}:rb={:.4}:gr={:.4}:gg={:.4}:gb={:.4}:br={:.4}:bg={:.4}:bb={:.4},",
                    lerp(0.393),
                    a * 0.769,
                    a * 0.189,
                    a * 0.349,
                    lerp(0.686),
                    a * 0.168,
                    a * 0.272,
                    a * 0.534,
                    lerp(0.131)
                )
                .ok();
            }
            Effect::Invert { amount } => {
                if *amount < 0.99 {
                    warnings.push(
                        "l'effet « inverser » est binaire : appliqué à pleine intensité".into(),
                    );
                }
                out.push_str("negate,");
            }
            Effect::Vignette { amount } => {
                write!(
                    out,
                    "vignette=angle={:.4},",
                    amount.clamp(0.0, std::f64::consts::PI)
                )
                .ok();
            }
            Effect::ChromaKey {
                color,
                similarity,
                blend,
            } => {
                write!(
                    out,
                    "chromakey=color={}:similarity={:.4}:blend={:.4},",
                    hex(color),
                    similarity.clamp(0.0, 1.0),
                    blend.clamp(0.0, 1.0)
                )
                .ok();
            }
        }
    }
    out
}

/// Build the `drawtext` chain for a text clip.
///
/// The text itself is passed via `textfile=`, so the only values that go through
/// the filtergraph parser are paths. That is deliberate: ffmpeg's graph escaping
/// is inconsistent (`\:` works, `%` needs `\\%`), and user-authored titles must
/// never be able to break a render.
fn drawtext_chain(text: &TextOverlay, text_path: &Path) -> String {
    let family = if text.font_family.trim().is_empty() {
        DEFAULT_FONT.to_string()
    } else {
        text.font_family.clone()
    };

    // A resolved font file is the only way to get real bold/italic out of builds
    // whose `drawtext` has no such options.
    let mut style = String::new();
    let font_selector = match crate::fonts::resolve(&family, text.bold, text.italic) {
        Some(path) => format!("fontfile='{}'", escape_graph(&path)),
        None => {
            if crate::fonts::drawtext_has_style_options() {
                if text.bold {
                    style.push_str(":bold=1");
                }
                if text.italic {
                    style.push_str(":italic=1");
                }
            }
            format!("font='{}'", escape_graph(&family))
        }
    };

    let background = match text.background {
        Some(bg) if bg.a > 0 => {
            format!(":box=1:boxcolor={}@{}:boxborderw=18", hex(&bg), alpha(&bg))
        }
        _ => String::new(),
    };
    format!(
        "drawtext={font_selector}:textfile='{}':fontsize={}:fontcolor={}@{}{}{}:x=(w-text_w)/2:y=(h-text_h)/2:line_spacing=8,",
        escape_graph(&text_path.to_string_lossy()),
        text.font_size.max(4),
        hex(&text.color),
        alpha(&text.color),
        style,
        background
    )
}

/// Escape a value that the *filtergraph parser* will read.
fn escape_graph(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 8);
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            ':' => out.push_str("\\:"),
            ',' => out.push_str("\\,"),
            '%' => out.push_str("\\%"),
            '[' => out.push_str("\\["),
            ']' => out.push_str("\\]"),
            ';' => out.push_str("\\;"),
            _ => out.push(ch),
        }
    }
    out
}

/// Escape the expansions `drawtext` itself performs on text content.
///
/// Only `%` is special (`%{pts}`, `%{n}`…). Everything else — newlines, quotes,
/// colons, brackets — is literal once the text arrives through a file.
fn escape_drawtext_expansions(content: &str) -> String {
    let mut out = String::with_capacity(content.len() + 8);
    for ch in content.chars() {
        if ch == '%' {
            out.push_str("\\%");
        } else {
            out.push(ch);
        }
    }
    out
}

/// FNV-1a: used only to name cache files, so a short, dependency-free hash is
/// the right tool.
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    hash
}

fn seconds(micros: Micros) -> String {
    format!("{:.6}", micros as f64 / ONE_SECOND as f64)
}

/// ffmpeg wants an exact rational here, not a decimal with a dot.
fn frame_rate(fps: f64) -> String {
    if (fps - fps.round()).abs() < 1e-6 {
        return format!("{}", fps.round().max(1.0) as i64);
    }
    let candidates: [(f64, &str); 4] = [
        (23.976, "24000/1001"),
        (29.97, "30000/1001"),
        (47.952, "48000/1001"),
        (59.94, "60000/1001"),
    ];
    for (value, rational) in candidates {
        if (fps - value).abs() < 0.01 {
            return rational.to_string();
        }
    }
    format!("{fps}")
}

fn hex(color: &Rgba) -> String {
    format!("0x{:02X}{:02X}{:02X}", color.r, color.g, color.b)
}

fn alpha(color: &Rgba) -> String {
    format!("{:.4}", color.a as f64 / 255.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn canvas() -> Canvas {
        Canvas {
            width: 1920,
            height: 1080,
            fps: 30.0,
            background: Rgba::BLACK,
        }
    }

    fn asset(id: &str) -> MediaAsset {
        MediaAsset {
            id: id.into(),
            name: format!("{id}.mp4"),
            path: format!("/tmp/{id}.mp4"),
            kind: AssetKind::Video,
            duration: 5 * ONE_SECOND,
            width: 1920,
            height: 1080,
            fps: 30.0,
            video_codec: Some("h264".into()),
            audio_codec: Some("aac".into()),
            audio_channels: 2,
            audio_sample_rate: 48000,
            size_bytes: 1024,
            thumbnail_path: None,
            preview_path: None,
            waveform: None,
            browser_playable: true,
        }
    }

    fn text_clip(content: &str, timeline: TimeRange) -> Clip {
        let mut clip = Clip::new("synth", TimeRange::new(0, timeline.duration), timeline);
        clip.text = Some(TextOverlay {
            content: content.into(),
            font_family: "DejaVu Sans".into(),
            font_size: 72,
            color: Rgba {
                r: 255,
                g: 255,
                b: 255,
                a: 255,
            },
            align: TextAlign::Center,
            background: None,
            bold: true,
            italic: false,
        });
        clip
    }

    #[test]
    fn graph_is_single_line_and_semicolon_separated() {
        // Regression guard: ffmpeg rejects a multi-line -filter_complex with
        // "Trailing garbage after a filter", which is a miserable bug to chase.
        let mut project = Project::new("test", canvas());
        project.assets.push(asset("a1"));
        project.tracks[0].clips.push(Clip::new(
            "a1",
            TimeRange::new(0, 2 * ONE_SECOND),
            TimeRange::new(0, 2 * ONE_SECOND),
        ));
        let plan = build_plan(&project, &RenderRange::whole(&project), 640, 360, 30.0);
        assert!(
            !plan.filter_complex.contains('\n'),
            "le graphe ne doit contenir aucun retour à la ligne"
        );
        assert!(plan.filter_complex.contains(";["));
        assert!(plan.filter_complex.ends_with(';'));
        assert!(plan.pretty().contains('\n'));
    }

    #[test]
    fn empty_project_still_produces_a_canvas() {
        let project = Project::new("vide", canvas());
        let plan = build_plan(&project, &RenderRange::whole(&project), 640, 360, 30.0);
        assert!(plan.filter_complex.contains("[vout]"));
        assert!(plan.filter_complex.contains("[aout]"));
        assert!(plan.filter_complex.contains("anullsrc"));
        assert!(plan.inputs.is_empty());
        assert!(plan
            .warnings
            .iter()
            .any(|w| w.contains("aucun clip visible")));
    }

    #[test]
    fn clip_is_gated_padded_and_delayed() {
        let mut project = Project::new("test", canvas());
        project.assets.push(asset("a1"));
        project.tracks[0].clips.push(Clip::new(
            "a1",
            TimeRange::new(0, 2 * ONE_SECOND),
            TimeRange::new(ONE_SECOND, 2 * ONE_SECOND),
        ));
        let plan = build_plan(&project, &RenderRange::whole(&project), 640, 360, 30.0);
        let g = &plan.filter_complex;
        assert!(g.contains("tpad=start_duration=1.000000"));
        assert!(g.contains("enable='between(t,1.000000,3.000000 )'"));
        assert!(g.contains("overlay="));
        assert!(g.contains("format=yuv420p[vout]"));
        assert!(g.contains("adelay=1000:all=1"));
        assert_eq!(plan.inputs.len(), 1);
    }

    #[test]
    fn placement_is_never_rebased_after_padding() {
        // Regression guard: a trailing setpts=PTS-STARTPTS after tpad slides the
        // clip in time and makes the enable gate show the wrong frames.
        let mut project = Project::new("test", canvas());
        project.assets.push(asset("a1"));
        project.tracks[0].clips.push(Clip::new(
            "a1",
            TimeRange::new(0, 2 * ONE_SECOND),
            TimeRange::new(ONE_SECOND, 2 * ONE_SECOND),
        ));
        let plan = build_plan(&project, &RenderRange::whole(&project), 640, 360, 30.0);
        assert!(!plan.filter_complex.contains("setpts=PTS-STARTPTS[ov"));
        assert!(plan.filter_complex.contains("trim=end=3.000000[ov0]"));
    }

    #[test]
    fn render_range_trims_and_rebases() {
        let mut project = Project::new("test", canvas());
        project.assets.push(asset("a1"));
        project.tracks[0].clips.push(Clip::new(
            "a1",
            TimeRange::new(0, 5 * ONE_SECOND),
            TimeRange::new(2 * ONE_SECOND, 3 * ONE_SECOND),
        ));
        let plan = build_plan(
            &project,
            &RenderRange {
                from: 3 * ONE_SECOND,
                to: 5 * ONE_SECOND,
            },
            640,
            360,
            30.0,
        );
        assert_eq!(plan.duration, 2 * ONE_SECOND);
        // One second into the source, two seconds long, starting at zero.
        assert!(plan
            .filter_complex
            .contains("trim=start=1.000000:duration=2.000000"));
        assert!(plan
            .filter_complex
            .contains("enable='between(t,0.000000,2.000000 )'"));
    }

    #[test]
    fn source_offset_scales_with_speed() {
        // A 2x clip seen 1 s into its timeline must read 2 s into the source.
        let mut project = Project::new("test", canvas());
        project.assets.push(asset("a1"));
        let mut clip = Clip::new(
            "a1",
            TimeRange::new(0, 10 * ONE_SECOND),
            TimeRange::new(0, 5 * ONE_SECOND),
        );
        clip.speed = 2.0;
        project.tracks[0].clips.push(clip);
        let plan = build_plan(
            &project,
            &RenderRange {
                from: ONE_SECOND,
                to: 2 * ONE_SECOND,
            },
            640,
            360,
            30.0,
        );
        // 1 s of clip time at 2x consumes 2 s of source, for 1 s of output.
        assert!(plan
            .filter_complex
            .contains("trim=start=2.000000:duration=2.000000"));
    }

    #[test]
    fn fades_are_expressed_in_clip_time() {
        // The clip is 4 s long with 1 s fades. A window in the middle must not
        // move them: fade out still starts at clip time 3 s.
        let mut project = Project::new("test", canvas());
        project.assets.push(asset("a1"));
        let mut clip = Clip::new(
            "a1",
            TimeRange::new(0, 4 * ONE_SECOND),
            TimeRange::new(0, 4 * ONE_SECOND),
        );
        clip.fade_in = ONE_SECOND;
        clip.fade_out = ONE_SECOND;
        project.tracks[0].clips.push(clip);

        let plan = build_plan(
            &project,
            &RenderRange {
                from: 2 * ONE_SECOND,
                to: 3 * ONE_SECOND,
            },
            640,
            360,
            30.0,
        );
        assert!(plan
            .filter_complex
            .contains("fade=t=in:st=0.000000:d=1.000000"));
        assert!(plan
            .filter_complex
            .contains("fade=t=out:st=3.000000:d=1.000000"));
        assert!(plan
            .filter_complex
            .contains("afade=t=in:st=0.000000:d=1.000000"));
        // The window enters the clip two seconds in, so the stream is shifted
        // into clip time and back out again.
        assert!(plan.filter_complex.contains("setpts=PTS+2.000000/TB"));
        assert!(plan.filter_complex.contains("setpts=PTS-2.000000/TB"));
        assert!(plan.filter_complex.contains("asetpts=PTS+2.000000/TB"));
    }

    #[test]
    fn a_window_at_the_clip_start_needs_no_shift() {
        let mut project = Project::new("test", canvas());
        project.assets.push(asset("a1"));
        project.tracks[0].clips.push(Clip::new(
            "a1",
            TimeRange::new(0, 4 * ONE_SECOND),
            TimeRange::new(0, 4 * ONE_SECOND),
        ));
        let plan = build_plan(&project, &RenderRange::whole(&project), 640, 360, 30.0);
        assert!(!plan.filter_complex.contains("/TB"));
    }

    #[test]
    fn fades_cannot_cross_in_the_middle() {
        // Two one-second fades on a one-second clip would leave no full-opacity
        // frame at all, so each is capped at half the clip.
        assert_eq!(fade_duration(ONE_SECOND, ONE_SECOND), 500_000);
        assert_eq!(
            fade_duration(2 * ONE_SECOND, 4 * ONE_SECOND),
            2 * ONE_SECOND
        );
        assert_eq!(fade_duration(0, 4 * ONE_SECOND), 0);
    }

    #[test]
    fn shared_asset_is_only_an_input_once() {
        let mut project = Project::new("test", canvas());
        project.assets.push(asset("a1"));
        for start in [0, 2 * ONE_SECOND] {
            project.tracks[0].clips.push(Clip::new(
                "a1",
                TimeRange::new(0, ONE_SECOND),
                TimeRange::new(start, ONE_SECOND),
            ));
        }
        let plan = build_plan(&project, &RenderRange::whole(&project), 640, 360, 30.0);
        assert_eq!(plan.inputs.len(), 1);
        assert!(plan.filter_complex.contains("amix=inputs=2"));
    }

    #[test]
    fn text_clip_needs_no_file_input() {
        let mut project = Project::new("test", canvas());
        project.tracks[0].clips.push(text_clip(
            "Salut : le monde, 100%",
            TimeRange::new(0, ONE_SECOND),
        ));
        let plan = build_plan(&project, &RenderRange::whole(&project), 640, 360, 30.0);
        assert!(plan.inputs.is_empty());
        assert!(plan.filter_complex.contains("drawtext="));
        // The text goes to a file, so the graph carries a path and no content.
        assert!(plan.filter_complex.contains("textfile="));
        assert!(!plan.filter_complex.contains("text='"));
        assert_eq!(plan.text_files.len(), 1);
        assert_eq!(plan.text_files[0].content, "Salut : le monde, 100\\%");
    }

    #[test]
    fn identical_text_reuses_one_file() {
        let mut project = Project::new("test", canvas());
        for start in [0, 2 * ONE_SECOND] {
            project.tracks[0]
                .clips
                .push(text_clip("Titre", TimeRange::new(start, ONE_SECOND)));
        }
        let plan = build_plan(&project, &RenderRange::whole(&project), 640, 360, 30.0);
        assert_eq!(plan.text_files.len(), 1, "même contenu, un seul fichier");
    }

    #[test]
    fn text_in_a_sub_window_reads_from_its_own_start() {
        // The text raster is generated per clip, so entering the clip 0.5 s in
        // must trim the raster by 0.5 s rather than seek the asset.
        let mut project = Project::new("test", canvas());
        project.tracks[0]
            .clips
            .push(text_clip("Titre", TimeRange::new(0, 2 * ONE_SECOND)));
        let plan = build_plan(
            &project,
            &RenderRange {
                from: 500_000,
                to: 1_500_000,
            },
            640,
            360,
            30.0,
        );
        assert!(plan
            .filter_complex
            .contains("trim=start=0.500000:duration=1.000000"));
    }

    #[test]
    fn atempo_is_chained_for_extremes() {
        assert!(atempo_chain(1.0).is_empty());
        assert_eq!(atempo_chain(2.0), "atempo=2.000000,");
        assert!(atempo_chain(200.0).starts_with("atempo=100,"));
        assert!(atempo_chain(0.2).starts_with("atempo=0.5,"));
    }

    #[test]
    fn graph_escaper_covers_metacharacters() {
        assert_eq!(
            escape_graph("a:b,c'd%e[f];g\\h"),
            "a\\:b\\,c\\'d\\%e\\[f\\]\\;g\\\\h"
        );
    }

    #[test]
    fn only_percent_is_expanded_by_drawtext() {
        // Newlines, quotes and colons are literal once the text arrives by file.
        assert_eq!(
            escape_drawtext_expansions("100% d'accord\nligne 2: ok"),
            "100\\% d'accord\nligne 2: ok"
        );
    }

    #[test]
    fn text_file_names_are_content_addressed() {
        assert_eq!(fnv1a(b""), fnv1a(b""));
        assert_ne!(fnv1a(b"a"), fnv1a(b"b"));
    }

    #[test]
    fn frame_rates_prefer_exact_rationals() {
        assert_eq!(frame_rate(30.0), "30");
        assert_eq!(frame_rate(29.97), "30000/1001");
        assert_eq!(frame_rate(23.976), "24000/1001");
        assert_eq!(frame_rate(25.0), "25");
    }

    #[test]
    fn still_images_are_looped() {
        let mut project = Project::new("test", canvas());
        let mut image = asset("img");
        image.kind = AssetKind::Image;
        image.audio_codec = None;
        image.path = "/tmp/img.png".into();
        project.assets.push(image);
        project.tracks[0].clips.push(Clip::new(
            "img",
            TimeRange::new(0, 5 * ONE_SECOND),
            TimeRange::new(0, 5 * ONE_SECOND),
        ));
        let plan = build_plan(&project, &RenderRange::whole(&project), 640, 360, 30.0);
        assert!(plan.filter_complex.contains("loop=loop=-1:size=1:start=0"));
    }

    #[test]
    fn hardware_finish_replaces_the_pixel_format_step() {
        let project = Project::new("test", canvas());
        let mut target = RenderTarget::new(640, 360, 30.0);
        target.finish = Some("format=nv12,hwupload".into());
        let plan = build_plan_with(&project, &RenderRange::whole(&project), &target);
        assert!(plan.filter_complex.contains("format=nv12,hwupload[vout]"));
        assert!(!plan.filter_complex.contains("format=yuv420p[vout]"));
    }
}
