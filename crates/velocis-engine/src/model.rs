//! Core domain types for a non-linear video editor.
//!
//! Design notes (deliberate choices, all of them testable):
//!
//! * **Time is integer microseconds.** `f32`/`f64` seconds accumulate error
//!   across trim/split/ripple operations and make undo/redo comparisons
//!   unreliable. Integers survive round-trips through JSON and JS (`2^53`
//!   microseconds is ~285 years, so the wire format is safe).
//! * **A clip references an asset, never a file path.** Paths move; assets carry
//!   the stable identity and the probed media facts.
//! * **Tracks are ordered: index 0 renders first (bottom).** Composition order is
//!   derived from track order then clip start, so it is reproducible without any
//!   hidden state.
//! * **Clips on a single track never overlap.** Any operation that would create an
//!   overlap is resolved by the timeline ops module (`trim`, `split`, `move`),
//!   which keeps rendering trivial and matches how simple editors behave.

use serde::{Deserialize, Serialize};

pub type Micros = i64;
pub type ClipId = String;
pub type TrackId = String;
pub type AssetId = String;

/// Microseconds per second.
pub const ONE_SECOND: Micros = 1_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeRange {
    pub start: Micros,
    pub duration: Micros,
}

impl TimeRange {
    pub fn new(start: Micros, duration: Micros) -> Self {
        Self {
            start,
            duration: duration.max(0),
        }
    }
    pub fn end(&self) -> Micros {
        self.start + self.duration
    }
    pub fn contains(&self, t: Micros) -> bool {
        t >= self.start && t < self.end()
    }
}

/// The output canvas. Also the reference frame for all transforms.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Canvas {
    pub width: u32,
    pub height: u32,
    /// Output frame rate. Kept as `f64` because broadcast rates are 23.976/29.97.
    pub fps: f64,
    pub background: Rgba,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub const BLACK: Rgba = Rgba {
        r: 0,
        g: 0,
        b: 0,
        a: 255,
    };
    pub fn to_hex(&self) -> String {
        format!("0x{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }
}

/// Aspect-ratio presets exposed by the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AspectRatio {
    Landscape16x9,
    Portrait9x16,
    Square1x1,
    Portrait4x5,
    Cinematic21x9,
}

impl AspectRatio {
    pub fn dimensions(self, height: u32) -> (u32, u32) {
        // Always even numbers: H.264/HEVC require even dimensions.
        let even = |v: f64| ((v / 2.0).round() * 2.0).max(2.0) as u32;
        match self {
            AspectRatio::Landscape16x9 => (even(height as f64 * 16.0 / 9.0), height),
            AspectRatio::Portrait9x16 => (height, even(height as f64 * 16.0 / 9.0)),
            AspectRatio::Square1x1 => (height, height),
            AspectRatio::Portrait4x5 => (even(height as f64 * 4.0 / 5.0), height),
            AspectRatio::Cinematic21x9 => (even(height as f64 * 21.0 / 9.0), height),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub canvas: Canvas,
    pub tracks: Vec<Track>,
    pub assets: Vec<MediaAsset>,
    pub created_at: String,
    pub modified_at: String,
}

pub const SCHEMA_VERSION: u32 = 1;

impl Project {
    pub fn new(name: impl Into<String>, canvas: Canvas) -> Self {
        let now = crate::now_rfc3339();
        Self {
            schema_version: SCHEMA_VERSION,
            id: uuid::Uuid::new_v4().to_string(),
            name: name.into(),
            canvas,
            tracks: vec![Track::new(TrackKind::Video, "Vidéo 1")],
            assets: Vec::new(),
            created_at: now.clone(),
            modified_at: now,
        }
    }

    /// Total duration = furthest clip end across every track.
    pub fn duration(&self) -> Micros {
        self.tracks
            .iter()
            .flat_map(|t| t.clips.iter())
            .map(|c| c.timeline.end())
            .max()
            .unwrap_or(0)
    }

    pub fn asset(&self, id: &str) -> Option<&MediaAsset> {
        self.assets.iter().find(|a| a.id == id)
    }

    pub fn track(&self, id: &str) -> Option<&Track> {
        self.tracks.iter().find(|t| t.id == id)
    }

    pub fn track_mut(&mut self, id: &str) -> Option<&mut Track> {
        self.tracks.iter_mut().find(|t| t.id == id)
    }

    pub fn clip(&self, id: &str) -> Option<(&Track, &Clip)> {
        self.tracks
            .iter()
            .find_map(|t| t.clips.iter().find(|c| c.id == id).map(|c| (t, c)))
    }

    pub fn touch(&mut self) {
        self.modified_at = crate::now_rfc3339();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackKind {
    Video,
    Audio,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Track {
    pub id: TrackId,
    pub kind: TrackKind,
    pub name: String,
    pub muted: bool,
    pub locked: bool,
    pub hidden: bool,
    /// Clips sorted by `timeline.start`, non-overlapping.
    pub clips: Vec<Clip>,
}

impl Track {
    pub fn new(kind: TrackKind, name: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            kind,
            name: name.into(),
            muted: false,
            locked: false,
            hidden: false,
            clips: Vec::new(),
        }
    }

    /// Sort and drop zero-length clips. Call after any structural change.
    pub fn normalize(&mut self) {
        self.clips
            .retain(|c| c.timeline.duration > 0 && c.source.duration > 0);
        self.clips.sort_by_key(|c| c.timeline.start);
    }

    pub fn clip(&self, id: &str) -> Option<&Clip> {
        self.clips.iter().find(|c| c.id == id)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Clip {
    pub id: ClipId,
    pub asset_id: AssetId,
    /// Which part of the source asset is used.
    pub source: TimeRange,
    /// Where it sits on the timeline.
    pub timeline: TimeRange,
    /// Playback rate multiplier. 1.0 = original speed.
    pub speed: f64,
    pub opacity: f64,
    /// Linear audio gain, 0.0 = mute, 1.0 = unity.
    pub volume: f64,
    pub transform: Transform,
    pub fade_in: Micros,
    pub fade_out: Micros,
    pub effects: Vec<Effect>,
    /// Optional text payload for title/sticker clips.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<TextOverlay>,
    /// Transitions on the clip's edges.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transition_in: Option<Transition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transition_out: Option<Transition>,
}

impl Clip {
    pub fn new(asset_id: impl Into<String>, source: TimeRange, timeline: TimeRange) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            asset_id: asset_id.into(),
            source,
            timeline,
            speed: 1.0,
            opacity: 1.0,
            volume: 1.0,
            transform: Transform::default(),
            fade_in: 0,
            fade_out: 0,
            effects: Vec::new(),
            text: None,
            transition_in: None,
            transition_out: None,
        }
    }

    pub fn is_text(&self) -> bool {
        self.text.is_some()
    }
}

/// Transform expressed relative to the canvas centre, in canvas pixels.
///
/// `scale` is a multiplier applied to the clip's *fitted* size: 1.0 means
/// "contain inside the canvas, preserving aspect ratio". This keeps one number
/// meaningful across assets of different resolutions.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Transform {
    pub x: f64,
    pub y: f64,
    pub scale: f64,
    pub rotation_deg: f64,
    pub mirror_x: bool,
    pub mirror_y: bool,
    /// Crop insets as fractions of the source (0.0..0.5 each side).
    pub crop_left: f64,
    pub crop_right: f64,
    pub crop_top: f64,
    pub crop_bottom: f64,
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            scale: 1.0,
            rotation_deg: 0.0,
            mirror_x: false,
            mirror_y: false,
            crop_left: 0.0,
            crop_right: 0.0,
            crop_top: 0.0,
            crop_bottom: 0.0,
        }
    }
}

impl Transform {
    pub fn is_identity(&self) -> bool {
        self.scale == 1.0
            && self.rotation_deg == 0.0
            && self.x == 0.0
            && self.y == 0.0
            && !self.mirror_x
            && !self.mirror_y
            && self.crop_left == 0.0
            && self.crop_right == 0.0
            && self.crop_top == 0.0
            && self.crop_bottom == 0.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextOverlay {
    pub content: String,
    pub font_family: String,
    pub font_size: u32,
    pub color: Rgba,
    pub align: TextAlign,
    #[serde(default)]
    pub background: Option<Rgba>,
    #[serde(default)]
    pub bold: bool,
    #[serde(default)]
    pub italic: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextAlign {
    Left,
    Center,
    Right,
}

/// Effects map 1:1 onto ffmpeg filters so the render graph stays declarative and
/// inspectable: a project file alone is enough to reproduce an export.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Effect {
    Brightness {
        amount: f64,
    },
    Contrast {
        amount: f64,
    },
    Saturation {
        amount: f64,
    },
    Gamma {
        amount: f64,
    },
    Hue {
        degrees: f64,
    },
    /// Colour temperature in kelvin. 6500 K is neutral.
    Temperature {
        kelvin: f64,
    },
    Blur {
        amount: f64,
    },
    Sharpen {
        amount: f64,
    },
    Grayscale {
        amount: f64,
    },
    Sepia {
        amount: f64,
    },
    Invert {
        amount: f64,
    },
    Vignette {
        amount: f64,
    },
    /// Incrustation couleur (chroma key).
    ChromaKey {
        color: Rgba,
        similarity: f64,
        blend: f64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transition {
    pub kind: TransitionKind,
    pub duration: Micros,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransitionKind {
    CrossFade,
    FadeToBlack,
    WipeLeft,
    WipeRight,
    SlideUp,
    Circle,
}

/// A media file that has been imported and probed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaAsset {
    pub id: AssetId,
    pub name: String,
    pub path: String,
    pub kind: AssetKind,
    pub duration: Micros,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    pub audio_channels: u32,
    pub audio_sample_rate: u32,
    pub size_bytes: u64,
    /// Cache keys for generated derivatives.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumbnail_path: Option<String>,
    /// Seek-friendly proxy prepared for the live preview.
    ///
    /// Source files often carry keyframes several seconds apart, which makes
    /// scrubbing decode from the previous keyframe every time. A proxy with a
    /// keyframe every 12 frames turns that into an instant seek.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waveform: Option<Waveform>,
    /// Whether a browser can decode this file directly.
    ///
    /// Matroska and HEVC are the common cases where ffmpeg is happy and the
    /// browser is not: the live preview then needs proxy media, while the export
    /// keeps using the original. Projects written before this field existed are
    /// assumed playable, which is what their preview already relied on.
    #[serde(default = "default_true")]
    pub browser_playable: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetKind {
    Video,
    Audio,
    Image,
}

/// Audio peaks for timeline drawing: one (min,max) pair per bucket.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Waveform {
    pub samples_per_bucket: u32,
    pub peaks: Vec<(f32, f32)>,
}
