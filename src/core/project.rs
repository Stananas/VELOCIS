#![allow(dead_code)]

use std::path::PathBuf;
use std::time::SystemTime;

#[derive(Clone)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub path: PathBuf,
    pub created_at: SystemTime,
    pub last_opened: SystemTime,
    pub media: Vec<MediaItem>,
    pub sequences: Vec<Sequence>,
    pub current_sequence: usize,
}

impl Project {
    pub fn new(id: String, name: String) -> Self {
        Self {
            id, name,
            path: PathBuf::new(),
            created_at: SystemTime::now(),
            last_opened: SystemTime::now(),
            media: Vec::new(),
            sequences: vec![Sequence::new("Séquence 1")],
            current_sequence: 0,
        }
    }
}

#[derive(Clone)]
pub struct MediaItem {
    pub id: String,
    pub name: String,
    pub path: PathBuf,
    pub media_type: MediaType,
    pub duration_seconds: f64,
}

impl MediaItem {
    pub fn new(name: &str, media_type: MediaType) -> Self {
        Self {
            id: nanoid(),
            name: name.to_string(),
            path: PathBuf::new(),
            media_type,
            duration_seconds: 0.0,
        }
    }

    pub fn sample(media_type: MediaType) -> Self {
        let (name, duration) = match media_type {
            MediaType::Video => ("Clip video.mp4", 8.0),
            MediaType::Audio => ("Musique fond.wav", 30.0),
            MediaType::Image => ("Photo couv.png", 3.0),
            MediaType::Other => ("Animation.gif", 5.0),
        };
        let mut item = Self::new(name, media_type);
        item.duration_seconds = duration;
        item
    }
}

fn nanoid() -> String {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_nanos().to_string())
        .unwrap_or_default()
}

#[derive(Clone, PartialEq)]
pub enum MediaType {
    Video,
    Audio,
    Image,
    Other,
}

#[derive(Clone)]
pub struct Sequence {
    pub id: String,
    pub name: String,
    pub tracks: Vec<Track>,
}

impl Sequence {
    pub fn new(name: &str) -> Self {
        Self {
            id: nanoid(),
            name: name.to_string(),
            tracks: vec![
                Track::new("V1", TrackKind::Video),
                Track::new("A1", TrackKind::Audio),
                Track::new("T1", TrackKind::Text),
            ],
        }
    }
}

#[derive(Clone)]
pub struct Track {
    pub id: String,
    pub name: String,
    pub kind: TrackKind,
    pub clips: Vec<Clip>,
    pub height: f32,
}

impl Track {
    pub fn new(name: &str, kind: TrackKind) -> Self {
        Self {
            id: nanoid(),
            name: name.to_string(),
            kind,
            clips: Vec::new(),
            height: 48.0,
        }
    }

    pub fn add_clip(&mut self, media_name: &str, media_type: &MediaType, duration: f64, position: f64) {
        let color = match media_type {
            MediaType::Video => (0.08, 0.8, 0.35),
            MediaType::Audio => (0.55, 0.6, 0.35),
            MediaType::Image => (0.15, 0.5, 0.35),
            MediaType::Other => (0.0, 0.0, 0.35),
        };
        self.clips.push(Clip {
            id: nanoid(),
            media_id: String::new(),
            name: media_name.to_string(),
            start_offset: 0.0,
            duration,
            position,
            color,
        });
    }
}

#[derive(Clone, PartialEq)]
pub enum TrackKind {
    Video,
    Audio,
    Text,
}

#[derive(Clone)]
pub struct Clip {
    pub id: String,
    pub media_id: String,
    pub name: String,
    pub start_offset: f64,
    pub duration: f64,
    pub position: f64,
    pub color: (f32, f32, f32),
}
