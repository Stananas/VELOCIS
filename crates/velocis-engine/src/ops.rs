//! Timeline editing operations.
//!
//! Two deliberate design choices:
//!
//! * **Every mutation goes through [`Editor::apply`]**, which snapshots the
//!   project first. Undo/redo is therefore snapshot-based rather than
//!   command-inverse-based. A project is a few kilobytes of JSON, so the memory
//!   cost is negligible, and it removes an entire class of bugs (an inverse that
//!   is subtly wrong produces a corrupted document the user cannot recover
//!   from). Snapshots are capped, oldest first.
//! * **Overlaps are resolved by rippling, never by deleting.** Dropping or
//!   moving a clip onto occupied time pushes the displaced clips to the right.
//!   Nothing the user has created can silently disappear, and the operation is
//!   deterministic, so it is safely undoable.

use crate::model::*;
use anyhow::{bail, Result};

const UNDO_DEPTH: usize = 200;
/// Two clips closer than this are treated as touching, so 30 fps material does
/// not accumulate one-microsecond gaps after a drag.
const SNAP: Micros = 1_000;

pub struct Editor {
    pub project: Project,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
}

struct Snapshot {
    project: Project,
    label: String,
}

impl Editor {
    pub fn new(project: Project) -> Self {
        Self {
            project,
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }

    /// Run a mutation, recording it for undo. Clears the redo stack.
    ///
    /// A failed operation is rolled back entirely. Without that, an error raised
    /// halfway through a mutation would leave the document changed but with no
    /// undo entry to get back — the worst of both outcomes.
    pub fn apply<T>(
        &mut self,
        label: impl Into<String>,
        f: impl FnOnce(&mut Project) -> Result<T>,
    ) -> Result<T> {
        let snapshot = Snapshot {
            project: self.project.clone(),
            label: label.into(),
        };
        let value = match f(&mut self.project) {
            Ok(value) => value,
            Err(error) => {
                self.project = snapshot.project;
                return Err(error);
            }
        };
        self.project.touch();
        self.undo.push(snapshot);
        if self.undo.len() > UNDO_DEPTH {
            self.undo.remove(0);
        }
        self.redo.clear();
        Ok(value)
    }

    /// Mutation that should not be undoable (e.g. cursor position, selection).
    pub fn apply_transient<T>(&mut self, f: impl FnOnce(&mut Project) -> Result<T>) -> Result<T> {
        f(&mut self.project)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|s| s.label.as_str())
    }
    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|s| s.label.as_str())
    }

    pub fn undo(&mut self) -> bool {
        let Some(snapshot) = self.undo.pop() else {
            return false;
        };
        let current = Snapshot {
            project: std::mem::replace(&mut self.project, snapshot.project),
            label: snapshot.label,
        };
        self.redo.push(current);
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(snapshot) = self.redo.pop() else {
            return false;
        };
        let current = Snapshot {
            project: std::mem::replace(&mut self.project, snapshot.project),
            label: snapshot.label,
        };
        self.undo.push(current);
        true
    }
}

/// Import a media file, reusing an existing asset when the path repeats.
pub fn import_asset(project: &mut Project, asset: MediaAsset) -> AssetId {
    if let Some(existing) = project.assets.iter().find(|a| a.path == asset.path) {
        return existing.id.clone();
    }
    let id = asset.id.clone();
    project.assets.push(asset);
    id
}

pub fn add_track(project: &mut Project, kind: TrackKind, name: Option<String>) -> TrackId {
    let index = project.tracks.iter().filter(|t| t.kind == kind).count() + 1;
    let default_name = match kind {
        TrackKind::Video => format!("Vidéo {index}"),
        TrackKind::Audio => format!("Audio {index}"),
    };
    let track = Track::new(kind, name.unwrap_or(default_name));
    let id = track.id.clone();
    project.tracks.push(track);
    id
}

pub fn remove_track(project: &mut Project, track_id: &str) -> Result<()> {
    let before = project.tracks.len();
    project.tracks.retain(|t| t.id != track_id);
    if project.tracks.len() == before {
        bail!("piste introuvable: {track_id}");
    }
    // Never leave the user with nothing to edit on.
    if !project.tracks.iter().any(|t| t.kind == TrackKind::Video) {
        add_track(project, TrackKind::Video, None);
    }
    Ok(())
}

/// Append a clip at the end of a track (the "add to timeline" behaviour).
pub fn append_clip(
    project: &mut Project,
    track_id: &str,
    asset_id: &str,
    source: Option<TimeRange>,
) -> Result<ClipId> {
    let asset = project
        .asset(asset_id)
        .ok_or_else(|| anyhow::anyhow!("média introuvable: {asset_id}"))?
        .clone();
    let track = project
        .track(track_id)
        .ok_or_else(|| anyhow::anyhow!("piste introuvable: {track_id}"))?;
    if track.locked {
        bail!("la piste est verrouillée");
    }
    let start = track
        .clips
        .iter()
        .map(|c| c.timeline.end())
        .max()
        .unwrap_or(0);
    let source = source.unwrap_or(TimeRange::new(0, asset.duration));
    let clip = Clip::new(
        &asset.id,
        source,
        TimeRange::new(start, (source.duration as f64 / 1.0) as Micros),
    );
    let id = clip.id.clone();
    let track = project.track_mut(track_id).expect("vérifié plus haut");
    track.clips.push(clip);
    track.normalize();
    Ok(id)
}

/// Place a clip at an explicit position, rippling whatever it lands on.
pub fn place_clip(
    project: &mut Project,
    track_id: &str,
    asset_id: &str,
    source: TimeRange,
    timeline_start: Micros,
) -> Result<ClipId> {
    let track = project
        .track(track_id)
        .ok_or_else(|| anyhow::anyhow!("piste introuvable: {track_id}"))?;
    if track.locked {
        bail!("la piste est verrouillée");
    }
    if !project.assets.iter().any(|a| a.id == asset_id) {
        bail!("média introuvable: {asset_id}");
    }
    let mut clip = Clip::new(
        asset_id,
        source,
        TimeRange::new(timeline_start.max(0), source.duration),
    );
    // The track's own kind decides how it is treated while compositing; a clip
    // dropped on an audio track keeps only its audio.
    clip.transform = Transform::default();
    let id = clip.id.clone();
    let track = project.track_mut(track_id).expect("vérifié plus haut");
    track.clips.push(clip);
    resolve_overlaps(track, &id);
    Ok(id)
}

/// Place a text clip. Text is synthetic: it has no asset and no source timeline,
/// so it gets its own operation instead of pretending to be media.
pub fn place_text(
    project: &mut Project,
    track_id: &str,
    text: TextOverlay,
    timeline_start: Micros,
    duration: Micros,
) -> Result<ClipId> {
    let track = project
        .track(track_id)
        .ok_or_else(|| anyhow::anyhow!("piste introuvable: {track_id}"))?;
    if track.locked {
        bail!("la piste est verrouillée");
    }
    let duration = duration.max(33_000);
    let mut clip = Clip::new(
        SYNTHETIC_ASSET_ID,
        TimeRange::new(0, duration),
        TimeRange::new(timeline_start.max(0), duration),
    );
    clip.text = Some(text);
    let id = clip.id.clone();
    let track = project.track_mut(track_id).expect("vérifié plus haut");
    track.clips.push(clip);
    resolve_overlaps(track, &id);
    Ok(id)
}

/// The asset id a synthetic (text) clip reports. It never resolves to a file.
pub const SYNTHETIC_ASSET_ID: &str = "synthetique";

/// Resolve overlaps so that the clip the user just placed keeps the position
/// they dropped it on.
///
/// The rule is *the placed clip wins, displaced clips move right as whole
/// clips*. Nothing is trimmed or dropped behind the user's back, a gap may be
/// left where material was pushed away, and the outcome is a pure function of
/// the input — so it is safe to undo.
fn resolve_overlaps(track: &mut Track, priority_id: &str) {
    track.normalize();
    let Some(anchor_index) = track.clips.iter().position(|c| c.id == priority_id) else {
        return;
    };
    // Take the anchor out, then re-place every other clip in start order,
    // treating the anchor's span as occupied.
    let anchor = track.clips.remove(anchor_index);
    let anchor_start = anchor.timeline.start;
    let anchor_end = anchor.timeline.end();

    let mut cursor: Micros = 0;
    let mut placed: Vec<Clip> = Vec::with_capacity(track.clips.len() + 1);
    for mut clip in track.clips.drain(..) {
        let mut start = clip.timeline.start.max(cursor);
        let overlaps_anchor =
            start < anchor_end - SNAP && start + clip.timeline.duration > anchor_start + SNAP;
        if overlaps_anchor {
            start = anchor_end.max(cursor);
        }
        clip.timeline.start = start;
        cursor = clip.timeline.end();
        placed.push(clip);
    }
    placed.push(anchor);
    track.clips = placed;
    track.normalize();
}

/// Move a clip to another track and/or another time, rippling collisions.
pub fn move_clip(
    project: &mut Project,
    clip_id: &str,
    to_track: &str,
    to_start: Micros,
) -> Result<()> {
    let (from_track, clip) = project
        .clip(clip_id)
        .map(|(t, c)| (t.id.clone(), c.clone()))
        .ok_or_else(|| anyhow::anyhow!("clip introuvable: {clip_id}"))?;
    let target = project
        .track(to_track)
        .ok_or_else(|| anyhow::anyhow!("piste introuvable: {to_track}"))?;
    if target.locked {
        bail!("la piste est verrouillée");
    }
    // A video clip cannot live on an audio-only track.
    if target.kind == TrackKind::Audio {
        let asset = project.asset(&clip.asset_id);
        let is_audio_asset = asset.map(|a| a.kind == AssetKind::Audio).unwrap_or(false);
        if !is_audio_asset && !clip.is_text() {
            bail!("cette piste n'accepte que de l'audio");
        }
    }

    {
        let source = project.track_mut(&from_track).expect("vérifié plus haut");
        source.clips.retain(|c| c.id != clip_id);
    }
    {
        let target = project.track_mut(to_track).expect("vérifié plus haut");
        let mut moved = clip;
        moved.timeline = TimeRange::new(to_start.max(0), moved.timeline.duration);
        target.clips.push(moved);
        resolve_overlaps(target, clip_id);
    }
    Ok(())
}

/// Trim a clip. Edge-accurate: the timeline range and the source range stay in
/// sync, and the source is clamped to the asset so a trim can never point
/// outside the media.
pub fn trim_clip(
    project: &mut Project,
    clip_id: &str,
    new_timeline_start: Micros,
    new_timeline_duration: Micros,
    new_source_start: Micros,
) -> Result<()> {
    let asset_duration = {
        let (_, clip) = project
            .clip(clip_id)
            .ok_or_else(|| anyhow::anyhow!("clip introuvable: {clip_id}"))?;
        project
            .asset(&clip.asset_id)
            .map(|a| a.duration)
            .unwrap_or(clip.source.duration)
    };
    let (track_id, _) = project
        .clip(clip_id)
        .map(|(t, c)| (t.id.clone(), c.id.clone()))
        .ok_or_else(|| anyhow::anyhow!("clip introuvable: {clip_id}"))?;

    let speed;
    let is_text;
    {
        let (_, clip) = project.clip(clip_id).expect("vérifié plus haut");
        speed = clip.speed.max(0.01);
        is_text = clip.is_text();
    }

    let source_start = if is_text {
        0
    } else {
        new_source_start.clamp(0, (asset_duration - SNAP).max(0))
    };
    let max_duration = if is_text {
        new_timeline_duration
    } else {
        (((asset_duration - source_start) as f64) / speed).round() as Micros
    };
    let duration = new_timeline_duration.clamp(SNAP, max_duration.max(SNAP));

    let track = project
        .track_mut(&track_id)
        .ok_or_else(|| anyhow::anyhow!("piste introuvable: {track_id}"))?;
    let clip = track
        .clips
        .iter_mut()
        .find(|c| c.id == clip_id)
        .ok_or_else(|| anyhow::anyhow!("clip introuvable: {clip_id}"))?;
    clip.source = TimeRange::new(source_start, (duration as f64 * speed).round() as Micros);
    clip.timeline = TimeRange::new(new_timeline_start.max(0), duration);
    resolve_overlaps(track, clip_id);
    Ok(())
}

/// Split a clip at an absolute timeline position.
pub fn split_clip(project: &mut Project, clip_id: &str, at: Micros) -> Result<ClipId> {
    let (track_id, clip) = project
        .clip(clip_id)
        .map(|(t, c)| (t.id.clone(), c.clone()))
        .ok_or_else(|| anyhow::anyhow!("clip introuvable: {clip_id}"))?;
    // Refuse degenerate splits: landing on an edge would create a zero-length
    // clip and confuse the timeline.
    if at <= clip.timeline.start + SNAP || at >= clip.timeline.end() - SNAP {
        bail!("point de découpe hors du clip");
    }
    let offset = at - clip.timeline.start;
    let source_offset = ((offset as f64) * clip.speed).round() as Micros;

    let mut right = clip.clone();
    right.id = uuid::Uuid::new_v4().to_string();
    right.source = TimeRange::new(
        clip.source.start + source_offset,
        clip.source.duration - source_offset,
    );
    right.timeline = TimeRange::new(at, clip.timeline.duration - offset);
    // Edge decorations belong to the outer edges only.
    right.fade_in = 0;
    right.transition_in = None;

    let track = project
        .track_mut(&track_id)
        .ok_or_else(|| anyhow::anyhow!("piste introuvable: {track_id}"))?;
    let left = track
        .clips
        .iter_mut()
        .find(|c| c.id == clip_id)
        .ok_or_else(|| anyhow::anyhow!("clip introuvable: {clip_id}"))?;
    left.source = TimeRange::new(clip.source.start, source_offset);
    left.timeline = TimeRange::new(clip.timeline.start, offset);
    left.fade_out = 0;
    left.transition_out = None;

    let id = right.id.clone();
    track.clips.push(right);
    track.normalize();
    Ok(id)
}

/// Delete a clip. `ripple` closes the gap it leaves behind.
pub fn delete_clip(project: &mut Project, clip_id: &str, ripple: bool) -> Result<()> {
    let (track_id, clip) = project
        .clip(clip_id)
        .map(|(t, c)| (t.id.clone(), c.clone()))
        .ok_or_else(|| anyhow::anyhow!("clip introuvable: {clip_id}"))?;
    let track = project
        .track_mut(&track_id)
        .ok_or_else(|| anyhow::anyhow!("piste introuvable: {track_id}"))?;
    track.clips.retain(|c| c.id != clip_id);
    if ripple {
        let gap_end = clip.timeline.end();
        let gap = clip.timeline.duration;
        for other in track.clips.iter_mut() {
            if other.timeline.start >= gap_end - SNAP {
                other.timeline.start = (other.timeline.start - gap).max(0);
            }
        }
    }
    track.normalize();
    Ok(())
}

/// Duplicate a clip directly after itself.
pub fn duplicate_clip(project: &mut Project, clip_id: &str) -> Result<ClipId> {
    let (track_id, clip) = project
        .clip(clip_id)
        .map(|(t, c)| (t.id.clone(), c.clone()))
        .ok_or_else(|| anyhow::anyhow!("clip introuvable: {clip_id}"))?;
    let mut copy = clip.clone();
    copy.id = uuid::Uuid::new_v4().to_string();
    copy.timeline = TimeRange::new(clip.timeline.end(), clip.timeline.duration);
    let id = copy.id.clone();
    let track = project
        .track_mut(&track_id)
        .ok_or_else(|| anyhow::anyhow!("piste introuvable: {track_id}"))?;
    track.clips.push(copy);
    resolve_overlaps(track, &id);
    Ok(id)
}

/// Change a clip's playback speed, keeping the timeline duration consistent.
pub fn set_speed(project: &mut Project, clip_id: &str, speed: f64) -> Result<()> {
    let speed = speed.clamp(0.1, 20.0);
    let (track_id, clip) = project
        .clip(clip_id)
        .map(|(t, c)| (t.id.clone(), c.clone()))
        .ok_or_else(|| anyhow::anyhow!("clip introuvable: {clip_id}"))?;
    if clip.is_text() {
        bail!("la vitesse ne s'applique pas à un texte");
    }
    // The source range is fixed; a different speed changes how long it lasts.
    let new_duration = ((clip.source.duration as f64) / speed).round() as Micros;
    let track = project
        .track_mut(&track_id)
        .ok_or_else(|| anyhow::anyhow!("piste introuvable: {track_id}"))?;
    let clip = track
        .clips
        .iter_mut()
        .find(|c| c.id == clip_id)
        .ok_or_else(|| anyhow::anyhow!("clip introuvable: {clip_id}"))?;
    clip.speed = speed;
    clip.timeline = TimeRange::new(clip.timeline.start, new_duration.max(SNAP));
    resolve_overlaps(track, clip_id);
    Ok(())
}

/// Split every clip crossing `at` on the given tracks (or all of them).
pub fn split_all_at(project: &mut Project, at: Micros, track_ids: &[String]) -> Vec<ClipId> {
    let targets: Vec<ClipId> = project
        .tracks
        .iter()
        .filter(|t| track_ids.is_empty() || track_ids.contains(&t.id))
        .filter(|t| !t.locked)
        .flat_map(|t| t.clips.iter())
        .filter(|c| {
            c.timeline.contains(at) && at > c.timeline.start + SNAP && at < c.timeline.end() - SNAP
        })
        .map(|c| c.id.clone())
        .collect();
    targets
        .iter()
        .filter_map(|id| split_clip(project, id, at).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> Project {
        Project::new(
            "test",
            Canvas {
                width: 1920,
                height: 1080,
                fps: 30.0,
                background: Rgba::BLACK,
            },
        )
    }

    fn asset(project: &mut Project, id: &str, duration: Micros) {
        project.assets.push(MediaAsset {
            id: id.into(),
            name: format!("{id}.mp4"),
            path: format!("/tmp/{id}.mp4"),
            kind: AssetKind::Video,
            duration,
            width: 1920,
            height: 1080,
            fps: 30.0,
            video_codec: Some("h264".into()),
            audio_codec: Some("aac".into()),
            audio_channels: 2,
            audio_sample_rate: 48000,
            size_bytes: 1,
            thumbnail_path: None,
            preview_path: None,
            waveform: None,
            browser_playable: true,
        });
    }

    #[test]
    fn append_puts_clips_end_to_end() {
        let mut p = project();
        asset(&mut p, "a1", 5 * ONE_SECOND);
        let track = p.tracks[0].id.clone();
        append_clip(&mut p, &track, "a1", None).unwrap();
        append_clip(&mut p, &track, "a1", None).unwrap();
        let clips = &p.tracks[0].clips;
        assert_eq!(clips.len(), 2);
        assert_eq!(clips[0].timeline.start, 0);
        assert_eq!(clips[1].timeline.start, 5 * ONE_SECOND);
    }

    #[test]
    fn dropping_on_top_keeps_the_drop_point_and_pushes_the_rest() {
        let mut p = project();
        asset(&mut p, "a1", 5 * ONE_SECOND);
        let track = p.tracks[0].id.clone();
        append_clip(&mut p, &track, "a1", None).unwrap();
        // Drop a two-second clip at t=1s, in the middle of the existing one.
        place_clip(
            &mut p,
            &track,
            "a1",
            TimeRange::new(0, 2 * ONE_SECOND),
            ONE_SECOND,
        )
        .unwrap();
        let clips = &p.tracks[0].clips;
        assert_eq!(clips.len(), 2, "aucun clip ne doit disparaître");
        // The dropped clip owns the position the user chose.
        assert_eq!(
            clips[0].timeline,
            TimeRange::new(ONE_SECOND, 2 * ONE_SECOND)
        );
        // The displaced clip keeps its whole length and starts right after.
        assert_eq!(clips[1].timeline.start, 3 * ONE_SECOND);
        assert_eq!(clips[1].timeline.duration, 5 * ONE_SECOND);
        assert_eq!(clips[1].source, TimeRange::new(0, 5 * ONE_SECOND));
    }

    #[test]
    fn split_preserves_total_length_and_source_offsets() {
        let mut p = project();
        asset(&mut p, "a1", 10 * ONE_SECOND);
        let track = p.tracks[0].id.clone();
        let clip = append_clip(
            &mut p,
            &track,
            "a1",
            Some(TimeRange::new(ONE_SECOND, 4 * ONE_SECOND)),
        )
        .unwrap();
        split_clip(&mut p, &clip, 2 * ONE_SECOND).unwrap();
        let clips = &p.tracks[0].clips;
        assert_eq!(clips.len(), 2);
        // Split at 2 s: the left half covers source [1s,3s), the right [3s,5s).
        assert_eq!(clips[0].source, TimeRange::new(ONE_SECOND, 2 * ONE_SECOND));
        assert_eq!(
            clips[1].source,
            TimeRange::new(3 * ONE_SECOND, 2 * ONE_SECOND)
        );
        assert_eq!(
            clips[0].timeline.duration + clips[1].timeline.duration,
            4 * ONE_SECOND
        );
        assert_eq!(clips[1].timeline.start, 2 * ONE_SECOND);
    }

    #[test]
    fn split_respects_speed() {
        let mut p = project();
        asset(&mut p, "a1", 10 * ONE_SECOND);
        let track = p.tracks[0].id.clone();
        let clip = append_clip(
            &mut p,
            &track,
            "a1",
            Some(TimeRange::new(0, 4 * ONE_SECOND)),
        )
        .unwrap();
        set_speed(&mut p, &clip, 2.0).unwrap();
        // 4 s of source at 2x covers 2 s of timeline.
        assert_eq!(p.tracks[0].clips[0].timeline.duration, 2 * ONE_SECOND);
        // Splitting one second in therefore consumes two seconds of source.
        split_clip(&mut p, &clip, ONE_SECOND).unwrap();
        let clips = &p.tracks[0].clips;
        assert_eq!(clips[0].source.duration, 2 * ONE_SECOND);
        assert_eq!(clips[1].source.start, 2 * ONE_SECOND);
    }

    #[test]
    fn split_refuses_edges() {
        let mut p = project();
        asset(&mut p, "a1", 5 * ONE_SECOND);
        let track = p.tracks[0].id.clone();
        let clip = append_clip(&mut p, &track, "a1", None).unwrap();
        assert!(split_clip(&mut p, &clip, 0).is_err());
        assert!(split_clip(&mut p, &clip, 5 * ONE_SECOND).is_err());
        assert!(split_clip(&mut p, &clip, 5 * ONE_SECOND + 1).is_err());
    }

    #[test]
    fn trim_clamps_to_available_media() {
        let mut p = project();
        asset(&mut p, "a1", 5 * ONE_SECOND);
        let track = p.tracks[0].id.clone();
        let clip = append_clip(&mut p, &track, "a1", None).unwrap();
        // Ask for ten seconds of a five second source: clamp, do not lie.
        trim_clip(&mut p, &clip, 0, 10 * ONE_SECOND, 0).unwrap();
        assert_eq!(p.tracks[0].clips[0].timeline.duration, 5 * ONE_SECOND);
        // Ask to start beyond the end: clamp to the last microsecond available.
        trim_clip(&mut p, &clip, 0, 10 * ONE_SECOND, 10 * ONE_SECOND).unwrap();
        let c = &p.tracks[0].clips[0];
        assert!(c.source.start < 5 * ONE_SECOND);
        assert_eq!(c.source.duration + c.source.start, ONE_SECOND * 5);
    }

    #[test]
    fn delete_with_ripple_closes_the_gap() {
        let mut p = project();
        asset(&mut p, "a1", 2 * ONE_SECOND);
        let track = p.tracks[0].id.clone();
        let first = append_clip(&mut p, &track, "a1", None).unwrap();
        append_clip(&mut p, &track, "a1", None).unwrap();
        append_clip(&mut p, &track, "a1", None).unwrap();
        assert_eq!(p.tracks[0].clips[2].timeline.start, 4 * ONE_SECOND);
        delete_clip(&mut p, &first, true).unwrap();
        assert_eq!(p.tracks[0].clips.len(), 2);
        assert_eq!(p.tracks[0].clips[0].timeline.start, 0);
        assert_eq!(p.tracks[0].clips[1].timeline.start, 2 * ONE_SECOND);
    }

    #[test]
    fn place_text_never_overlaps_other_clips() {
        let mut p = project();
        asset(&mut p, "a1", 5 * ONE_SECOND);
        let track = p.tracks[0].id.clone();
        append_clip(&mut p, &track, "a1", None).unwrap();
        let text = TextOverlay {
            content: "Titre".into(),
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
        };
        // Dropped in the middle of an existing clip: the text wins its position.
        place_text(&mut p, &track, text, 2 * ONE_SECOND, ONE_SECOND).unwrap();
        let clips = &p.tracks[0].clips;
        assert_eq!(clips.len(), 2);
        assert_eq!(
            clips[0].timeline,
            TimeRange::new(2 * ONE_SECOND, ONE_SECOND)
        );
        assert!(clips[0].is_text());
        assert_eq!(clips[1].timeline.start, 3 * ONE_SECOND);
        assert_eq!(clips[1].timeline.duration, 5 * ONE_SECOND);
    }

    #[test]
    fn undo_restores_exact_state() {
        let mut p = project();
        asset(&mut p, "a1", 5 * ONE_SECOND);
        let track = p.tracks[0].id.clone();
        let mut editor = Editor::new(p);
        let clip = editor
            .apply("Ajouter un clip", |p| append_clip(p, &track, "a1", None))
            .unwrap();
        assert_eq!(editor.project.tracks[0].clips.len(), 1);
        editor
            .apply("Découper", |p| {
                split_clip(p, &clip, ONE_SECOND).map(|_| ())
            })
            .unwrap();
        assert_eq!(editor.project.tracks[0].clips.len(), 2);
        assert!(editor.undo());
        assert_eq!(editor.project.tracks[0].clips.len(), 1);
        assert!(editor.undo());
        assert_eq!(editor.project.tracks[0].clips.len(), 0);
        assert!(!editor.can_undo());
        assert!(editor.redo());
        assert_eq!(editor.project.tracks[0].clips.len(), 1);
        assert_eq!(editor.undo_label(), Some("Ajouter un clip"));
    }

    #[test]
    fn a_failed_mutation_leaves_no_trace() {
        let mut p = project();
        asset(&mut p, "a1", 5 * ONE_SECOND);
        let track = p.tracks[0].id.clone();
        let mut editor = Editor::new(p);
        editor
            .apply("Ajouter", |p| append_clip(p, &track, "a1", None))
            .unwrap();
        let before = serde_json::to_string(&editor.project).unwrap();

        // The clip id does not exist: the operation must fail *and* change nothing.
        let result = editor.apply("Découper", |p| {
            split_clip(p, "inconnu", ONE_SECOND).map(|_| ())
        });
        assert!(result.is_err());
        assert_eq!(serde_json::to_string(&editor.project).unwrap(), before);
        // And the failed attempt must not have consumed the undo stack.
        assert_eq!(editor.undo_label(), Some("Ajouter"));
    }

    #[test]
    fn a_new_action_clears_redo() {
        let mut p = project();
        asset(&mut p, "a1", 5 * ONE_SECOND);
        let track = p.tracks[0].id.clone();
        let mut editor = Editor::new(p);
        editor
            .apply("a", |p| append_clip(p, &track, "a1", None))
            .unwrap();
        editor.undo();
        assert!(editor.can_redo());
        editor
            .apply("b", |p| append_clip(p, &track, "a1", None))
            .unwrap();
        assert!(!editor.can_redo());
    }

    #[test]
    fn locked_track_refuses_edits() {
        let mut p = project();
        asset(&mut p, "a1", 5 * ONE_SECOND);
        let track = p.tracks[0].id.clone();
        p.tracks[0].locked = true;
        assert!(append_clip(&mut p, &track, "a1", None).is_err());
    }

    #[test]
    fn video_clip_cannot_move_to_an_audio_track() {
        let mut p = project();
        asset(&mut p, "a1", 5 * ONE_SECOND);
        let video = p.tracks[0].id.clone();
        let audio = add_track(&mut p, TrackKind::Audio, None);
        let clip = append_clip(&mut p, &video, "a1", None).unwrap();
        assert!(move_clip(&mut p, &clip, &audio, 0).is_err());
    }

    #[test]
    fn removing_the_last_video_track_recreates_one() {
        let mut p = project();
        let id = p.tracks[0].id.clone();
        remove_track(&mut p, &id).unwrap();
        assert_eq!(
            p.tracks
                .iter()
                .filter(|t| t.kind == TrackKind::Video)
                .count(),
            1
        );
    }

    #[test]
    fn split_all_skips_locked_tracks() {
        let mut p = project();
        asset(&mut p, "a1", 5 * ONE_SECOND);
        let track = p.tracks[0].id.clone();
        append_clip(&mut p, &track, "a1", None).unwrap();
        p.tracks[0].locked = true;
        assert!(split_all_at(&mut p, 2 * ONE_SECOND, &[]).is_empty());
    }
}
