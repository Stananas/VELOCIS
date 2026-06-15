#![allow(dead_code)]

use std::path::PathBuf;
use std::time::SystemTime;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct SaveData {
    recent_projects: Vec<ProjectEntry>,
}

#[derive(Serialize, Deserialize)]
struct ProjectEntry {
    id: String,
    name: String,
    last_opened_secs: u64,
}

#[derive(Serialize, Deserialize)]
struct MediaEntry {
    id: String,
    name: String,
    path: String,
    media_type: String,
    duration_seconds: f64,
}

#[derive(Serialize, Deserialize)]
struct ClipEntry {
    id: String,
    media_id: String,
    name: String,
    start_offset: f64,
    duration: f64,
    position: f64,
    color_h: f32,
    color_s: f32,
    color_l: f32,
}

#[derive(Serialize, Deserialize)]
struct TrackEntry {
    id: String,
    name: String,
    kind: String,
    clips: Vec<ClipEntry>,
    height: f32,
}

#[derive(Serialize, Deserialize)]
struct ProjectData {
    media: Vec<MediaEntry>,
    tracks: Vec<TrackEntry>,
    playhead_position: f64,
}

fn data_dir() -> PathBuf {
    if let Ok(appdata) = std::env::var("APPDATA") {
        let dir = PathBuf::from(appdata).join("velocis");
        let _ = std::fs::create_dir_all(&dir);
        dir
    } else {
        let dir = PathBuf::from("./.velocis");
        let _ = std::fs::create_dir_all(&dir);
        dir
    }
}

fn save_path() -> PathBuf { data_dir().join("projects.json") }

fn project_path(id: &str) -> PathBuf { data_dir().join(format!("{}.json", id)) }

pub fn load_recent_projects() -> Vec<super::state::ProjectMeta> {
    let path = save_path();
    if !path.exists() { return vec![]; }
    let content = match std::fs::read_to_string(&path) { Ok(c) => c, Err(_) => return vec![] };
    let data: SaveData = match serde_json::from_str(&content) { Ok(d) => d, Err(_) => return vec![] };
    data.recent_projects.iter().map(|e| {
        let dur = std::time::Duration::from_secs(e.last_opened_secs);
        super::state::ProjectMeta {
            id: e.id.clone(), name: e.name.clone(),
            path: PathBuf::new(),
            last_opened: SystemTime::UNIX_EPOCH + dur,
        }
    }).collect()
}

pub fn save_recent_projects(projects: &[super::state::ProjectMeta]) {
    let entries: Vec<ProjectEntry> = projects.iter().map(|p| {
        let secs = p.last_opened.duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        ProjectEntry { id: p.id.clone(), name: p.name.clone(), last_opened_secs: secs }
    }).collect();
    if let Ok(content) = serde_json::to_string_pretty(&SaveData { recent_projects: entries })
        && let Err(e) = std::fs::write(save_path(), content)
    {
        eprintln!("failed to save recent projects: {e}");
    }
}

fn media_type_str(t: &super::project::MediaType) -> &str {
    match t { super::project::MediaType::Video => "video", super::project::MediaType::Audio => "audio", super::project::MediaType::Image => "image", _ => "other" }
}

fn media_type_from_str(s: &str) -> super::project::MediaType {
    match s { "video" => super::project::MediaType::Video, "audio" => super::project::MediaType::Audio, "image" => super::project::MediaType::Image, _ => super::project::MediaType::Other }
}

fn track_kind_str(t: &super::project::TrackKind) -> &str {
    match t { super::project::TrackKind::Video => "video", super::project::TrackKind::Audio => "audio", super::project::TrackKind::Text => "text" }
}

fn track_kind_from_str(s: &str) -> super::project::TrackKind {
    match s { "video" => super::project::TrackKind::Video, "audio" => super::project::TrackKind::Audio, _ => super::project::TrackKind::Text }
}

pub fn save_project_data(id: &str, media: &[super::project::MediaItem], sequence: &super::project::Sequence, playhead: f64) {
    let media_entries: Vec<MediaEntry> = media.iter().map(|m| MediaEntry {
        id: m.id.clone(), name: m.name.clone(),
        path: m.path.to_string_lossy().to_string(),
        media_type: media_type_str(&m.media_type).to_string(),
        duration_seconds: m.duration_seconds,
    }).collect();

    let tracks: Vec<TrackEntry> = sequence.tracks.iter().map(|t| TrackEntry {
        id: t.id.clone(), name: t.name.clone(),
        kind: track_kind_str(&t.kind).to_string(),
        height: t.height,
        clips: t.clips.iter().map(|c| ClipEntry {
            id: c.id.clone(), media_id: c.media_id.clone(), name: c.name.clone(),
            start_offset: c.start_offset, duration: c.duration, position: c.position,
            color_h: c.color.0, color_s: c.color.1, color_l: c.color.2,
        }).collect(),
    }).collect();

    let data = ProjectData { media: media_entries, tracks, playhead_position: playhead };
    if let Ok(content) = serde_json::to_string_pretty(&data)
        && let Err(e) = std::fs::write(project_path(id), content)
    {
        eprintln!("failed to save project {id}: {e}");
    }
}

pub fn load_project_data(id: &str) -> Option<(Vec<super::project::MediaItem>, super::project::Sequence, f64)> {
    let path = project_path(id);
    if !path.exists() { return None; }
    let content = std::fs::read_to_string(&path).ok()?;
    let data: ProjectData = serde_json::from_str(&content).ok()?;

    let media: Vec<super::project::MediaItem> = data.media.iter().map(|m| super::project::MediaItem {
        id: m.id.clone(), name: m.name.clone(),
        path: PathBuf::from(&m.path),
        media_type: media_type_from_str(&m.media_type),
        duration_seconds: m.duration_seconds,
    }).collect();

    let tracks: Vec<super::project::Track> = data.tracks.iter().map(|t| super::project::Track {
        id: t.id.clone(), name: t.name.clone(),
        kind: track_kind_from_str(&t.kind),
        height: t.height,
        clips: t.clips.iter().map(|c| super::project::Clip {
            id: c.id.clone(), media_id: c.media_id.clone(), name: c.name.clone(),
            start_offset: c.start_offset, duration: c.duration, position: c.position,
            color: (c.color_h, c.color_s, c.color_l),
        }).collect(),
    }).collect();

    let seq = super::project::Sequence {
        id: format!("seq_{}", id), name: "Sequence 1".to_string(), tracks,
    };

    Some((media, seq, data.playhead_position))
}
