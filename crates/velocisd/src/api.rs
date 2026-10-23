//! HTTP handlers. Thin: translate a request, call the engine, return a snapshot.

use crate::state::{App, ExportReportLite, Job, JobStatus, Snapshot};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tower_http::services::ServeFile;
use velocis_engine::export::{self, Container, Quality};
use velocis_engine::model::*;
use velocis_engine::ops;

type AppState = State<Arc<App>>;

/// Engine errors become 400s with the message intact: they are written for the
/// user ("la piste est verrouillée"), not for a log file.
pub struct ApiError(anyhow::Error);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let message = self.0.to_string();
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": message })),
        )
            .into_response()
    }
}

impl<E: Into<anyhow::Error>> From<E> for ApiError {
    fn from(value: E) -> Self {
        ApiError(value.into())
    }
}

type ApiResult<T> = Result<Json<T>, ApiError>;

#[derive(Serialize)]
pub struct Ok_ {
    ok: bool,
}

pub async fn state(State(app): AppState) -> ApiResult<Snapshot> {
    Ok(Json(app.snapshot()))
}

#[derive(Serialize)]
pub struct Capabilities {
    pub encoder: export::Encoder,
    pub hardware_accelerated: bool,
    pub containers: Vec<&'static str>,
    pub qualities: Vec<&'static str>,
    pub heights: Vec<u32>,
    pub aspects: Vec<&'static str>,
}

pub async fn capabilities() -> ApiResult<Capabilities> {
    let caps = export::capabilities();
    Ok(Json(Capabilities {
        encoder: caps.encoder,
        hardware_accelerated: caps.hardware_accelerated,
        containers: caps.containers,
        qualities: vec!["draft", "standard", "high", "best"],
        heights: vec![480, 720, 1080, 2160],
        aspects: vec!["landscape16x9", "portrait9x16", "square1x1", "portrait4x5"],
    }))
}

// ---------------------------------------------------------------- project

#[derive(Deserialize)]
pub struct NewProject {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub aspect: Option<AspectRatio>,
    #[serde(default)]
    pub height: Option<u32>,
}

pub async fn project_new(
    State(app): AppState,
    Json(body): Json<NewProject>,
) -> ApiResult<Snapshot> {
    let height = body.height.unwrap_or(1080).clamp(240, 2160);
    let aspect = body.aspect.unwrap_or(AspectRatio::Landscape16x9);
    let (width, height) = aspect.dimensions(height);
    let canvas = Canvas {
        width,
        height,
        fps: 30.0,
        background: Rgba {
            r: 12,
            g: 12,
            b: 18,
            a: 255,
        },
    };
    {
        let mut editor = app.editor.lock().unwrap();
        *editor = ops::Editor::new(Project::new(
            body.name.unwrap_or_else(|| "Projet sans titre".into()),
            canvas,
        ));
    }
    *app.project_path.lock().unwrap() = None;
    Ok(Json(app.snapshot()))
}

#[derive(Deserialize)]
pub struct SaveProject {
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Serialize)]
pub struct SaveResult {
    pub path: String,
}

/// Rename without touching anything else.
///
/// Deliberately separate from "new project": a rename must never be able to
/// discard a timeline, and making that impossible at the API level is cheaper
/// than trusting every caller to preserve it.
#[derive(Deserialize)]
pub struct RenameProject {
    pub name: String,
}

pub async fn project_rename(
    State(app): AppState,
    Json(body): Json<RenameProject>,
) -> ApiResult<Snapshot> {
    let name = body.name.trim().to_string();
    if name.is_empty() {
        return Err(ApiError(anyhow::anyhow!("le nom ne peut pas être vide")));
    }
    app.mutate("Renommer le projet", move |p| {
        p.name = name;
        Ok(())
    })?;
    Ok(Json(app.snapshot()))
}

/// Change the output canvas (aspect ratio, resolution) while keeping every clip.
#[derive(Deserialize)]
pub struct CanvasRequest {
    pub aspect: AspectRatio,
    pub height: u32,
}

pub async fn project_canvas(
    State(app): AppState,
    Json(body): Json<CanvasRequest>,
) -> ApiResult<Snapshot> {
    let height = body.height.clamp(240, 2160);
    let (width, height) = body.aspect.dimensions(height);
    app.mutate("Changer le format", move |p| {
        p.canvas.width = width;
        p.canvas.height = height;
        Ok(())
    })?;
    Ok(Json(app.snapshot()))
}

pub async fn project_save(
    State(app): AppState,
    Json(body): Json<SaveProject>,
) -> ApiResult<SaveResult> {
    let path = match body.path {
        Some(p) => PathBuf::from(p),
        None => app
            .project_path
            .lock()
            .unwrap()
            .clone()
            .unwrap_or_else(|| app.media_dir.join("projet.json")),
    };
    let project = app.project();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, serde_json::to_string_pretty(&project)?)?;
    *app.project_path.lock().unwrap() = Some(path.clone());
    Ok(Json(SaveResult {
        path: path.to_string_lossy().into_owned(),
    }))
}

#[derive(Deserialize)]
pub struct OpenProject {
    pub path: String,
}

pub async fn project_open(
    State(app): AppState,
    Json(body): Json<OpenProject>,
) -> ApiResult<Snapshot> {
    let path = PathBuf::from(&body.path);
    let text = std::fs::read_to_string(&path)?;
    let project: Project = serde_json::from_str(&text)?;
    {
        let mut editor = app.editor.lock().unwrap();
        *editor = ops::Editor::new(project);
    }
    *app.project_path.lock().unwrap() = Some(path);
    Ok(Json(app.snapshot()))
}

// ----------------------------------------------------------------- import

#[derive(Deserialize)]
pub struct ImportRequest {
    pub paths: Vec<String>,
}

#[derive(Serialize)]
pub struct ImportResult {
    pub imported: Vec<MediaAsset>,
    pub failed: Vec<FailedImport>,
    pub session: crate::state::SessionInfo,
}

#[derive(Serialize)]
pub struct FailedImport {
    pub path: String,
    pub error: String,
}

pub async fn import(
    State(app): AppState,
    Json(body): Json<ImportRequest>,
) -> ApiResult<ImportResult> {
    let mut imported = Vec::new();
    let mut failed = Vec::new();
    for raw in body.paths {
        let path = PathBuf::from(&raw);
        match crate::state::import_file(&app, &path) {
            Ok(asset) => {
                // Recorded through the editor so an import is undoable and the
                // asset lands in the saved project.
                let asset_for_project = asset.clone();
                app.mutate("Importer un média", move |p| {
                    ops::import_asset(p, asset_for_project);
                    Ok(())
                })?;
                imported.push(asset);
            }
            Err(err) => failed.push(FailedImport {
                path: raw,
                error: err.to_string(),
            }),
        }
    }
    let editor = app.editor.lock().unwrap();
    Ok(Json(ImportResult {
        imported,
        failed,
        session: crate::state::session_info(&editor),
    }))
}

/// Upload a file body and import it.
///
/// A browser cannot hand out the path of a picked file, so the bytes travel to
/// the service instead. The file is written once into the media directory and
/// then treated exactly like a file imported by path.
pub async fn upload(
    State(app): AppState,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> ApiResult<ImportResult> {
    let filename = headers
        .get("x-filename")
        .and_then(|v| v.to_str().ok())
        .map(|v| {
            // Keep the name, drop any directory component a client might send.
            v.rsplit(['/', '\\']).next().unwrap_or("media").to_string()
        })
        .unwrap_or_else(|| "media".to_string());
    if body.is_empty() {
        return Err(ApiError(anyhow::anyhow!("fichier vide")));
    }

    let safe: String = filename
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || " ._-()".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    let dest = app.media_dir.join("uploads").join(&safe);
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&dest, &body)?;

    match crate::state::import_file(&app, &dest) {
        Ok(asset) => {
            let asset_for_project = asset.clone();
            app.mutate("Importer un média", move |p| {
                ops::import_asset(p, asset_for_project);
                Ok(())
            })?;
            let editor = app.editor.lock().unwrap();
            Ok(Json(ImportResult {
                imported: vec![asset],
                failed: Vec::new(),
                session: crate::state::session_info(&editor),
            }))
        }
        Err(err) => {
            let _ = std::fs::remove_file(&dest);
            Err(ApiError(err))
        }
    }
}

// ------------------------------------------------------------------- clips

#[derive(Deserialize)]
pub struct AppendClip {
    pub track_id: String,
    pub asset_id: String,
    #[serde(default)]
    pub source_start: Option<Micros>,
    #[serde(default)]
    pub source_duration: Option<Micros>,
}

pub async fn clip_append(
    State(app): AppState,
    Json(body): Json<AppendClip>,
) -> ApiResult<Snapshot> {
    // A caller may specify only one end of the source range: fill the other from
    // the asset rather than silently ignoring the request.
    let asset = app
        .project()
        .asset(&body.asset_id)
        .ok_or_else(|| ApiError(anyhow::anyhow!("média introuvable")))?
        .clone();
    let source = if body.source_start.is_some() || body.source_duration.is_some() {
        Some(TimeRange::new(
            body.source_start
                .unwrap_or(0)
                .clamp(0, asset.duration.max(0)),
            body.source_duration.unwrap_or(asset.duration).max(33_000),
        ))
    } else {
        None
    };
    app.mutate("Ajouter un clip", move |p| {
        ops::append_clip(p, &body.track_id, &body.asset_id, source)
    })?;
    Ok(Json(app.snapshot()))
}

#[derive(Deserialize)]
pub struct PlaceClip {
    pub track_id: String,
    pub asset_id: String,
    pub timeline_start: Micros,
    #[serde(default)]
    pub source_start: Option<Micros>,
    #[serde(default)]
    pub source_duration: Option<Micros>,
}

pub async fn clip_place(State(app): AppState, Json(body): Json<PlaceClip>) -> ApiResult<Snapshot> {
    let project = app.project();
    let asset = project
        .asset(&body.asset_id)
        .ok_or_else(|| ApiError(anyhow::anyhow!("média introuvable")))?
        .clone();
    let source = TimeRange::new(
        body.source_start.unwrap_or(0),
        body.source_duration.unwrap_or(asset.duration),
    );
    app.mutate("Placer un clip", move |p| {
        ops::place_clip(
            p,
            &body.track_id,
            &body.asset_id,
            source,
            body.timeline_start,
        )
    })?;
    Ok(Json(app.snapshot()))
}

#[derive(Deserialize)]
pub struct MoveClip {
    pub clip_id: String,
    pub track_id: String,
    pub timeline_start: Micros,
}

pub async fn clip_move(State(app): AppState, Json(body): Json<MoveClip>) -> ApiResult<Snapshot> {
    app.mutate("Déplacer un clip", move |p| {
        ops::move_clip(p, &body.clip_id, &body.track_id, body.timeline_start)
    })?;
    Ok(Json(app.snapshot()))
}

#[derive(Deserialize)]
pub struct TrimClip {
    pub clip_id: String,
    pub timeline_start: Micros,
    pub timeline_duration: Micros,
    #[serde(default)]
    pub source_start: Option<Micros>,
}

pub async fn clip_trim(State(app): AppState, Json(body): Json<TrimClip>) -> ApiResult<Snapshot> {
    let source_start = match body.source_start {
        Some(v) => v,
        None => app
            .project()
            .clip(&body.clip_id)
            .map(|(_, c)| c.source.start)
            .ok_or_else(|| ApiError(anyhow::anyhow!("clip introuvable")))?,
    };
    app.mutate("Rogner un clip", move |p| {
        ops::trim_clip(
            p,
            &body.clip_id,
            body.timeline_start,
            body.timeline_duration,
            source_start,
        )
    })?;
    Ok(Json(app.snapshot()))
}

#[derive(Deserialize)]
pub struct SplitClip {
    pub clip_id: String,
    pub at: Micros,
}

pub async fn clip_split(State(app): AppState, Json(body): Json<SplitClip>) -> ApiResult<Snapshot> {
    app.mutate("Découper un clip", move |p| {
        ops::split_clip(p, &body.clip_id, body.at).map(|_| ())
    })?;
    Ok(Json(app.snapshot()))
}

#[derive(Deserialize)]
pub struct SplitAll {
    pub at: Micros,
    #[serde(default)]
    pub track_ids: Vec<String>,
}

pub async fn clip_split_all(
    State(app): AppState,
    Json(body): Json<SplitAll>,
) -> ApiResult<Snapshot> {
    app.mutate("Découper", move |p| {
        ops::split_all_at(p, body.at, &body.track_ids);
        Ok(())
    })?;
    Ok(Json(app.snapshot()))
}

#[derive(Deserialize)]
pub struct DeleteClip {
    pub clip_id: String,
    #[serde(default = "default_true")]
    pub ripple: bool,
}

fn default_true() -> bool {
    true
}

pub async fn clip_delete(
    State(app): AppState,
    Json(body): Json<DeleteClip>,
) -> ApiResult<Snapshot> {
    app.mutate("Supprimer un clip", move |p| {
        ops::delete_clip(p, &body.clip_id, body.ripple)
    })?;
    Ok(Json(app.snapshot()))
}

#[derive(Deserialize)]
pub struct ClipRef {
    pub clip_id: String,
}

pub async fn clip_duplicate(
    State(app): AppState,
    Json(body): Json<ClipRef>,
) -> ApiResult<Snapshot> {
    app.mutate("Dupliquer un clip", move |p| {
        ops::duplicate_clip(p, &body.clip_id).map(|_| ())
    })?;
    Ok(Json(app.snapshot()))
}

/// Partial update of a clip's properties. Every field is optional so the UI can
/// send only what the user actually dragged.
#[derive(Deserialize)]
pub struct ClipPatch {
    #[serde(default)]
    pub opacity: Option<f64>,
    #[serde(default)]
    pub volume: Option<f64>,
    #[serde(default)]
    pub speed: Option<f64>,
    #[serde(default)]
    pub fade_in: Option<Micros>,
    #[serde(default)]
    pub fade_out: Option<Micros>,
    #[serde(default)]
    pub transform: Option<Transform>,
    #[serde(default)]
    pub effects: Option<Vec<Effect>>,
    /// Replaces the text payload wholesale: the UI always sends the full object,
    /// which keeps "toggle bold" and "type a letter" the same code path.
    #[serde(default)]
    pub text: Option<TextOverlay>,
}

#[derive(Deserialize)]
pub struct UpdateClip {
    pub clip_id: String,
    #[serde(flatten)]
    pub patch: ClipPatch,
    #[serde(default)]
    pub label: Option<String>,
}

pub async fn clip_update(
    State(app): AppState,
    Json(body): Json<UpdateClip>,
) -> ApiResult<Snapshot> {
    let label = body
        .label
        .clone()
        .unwrap_or_else(|| "Modifier un clip".into());
    let clip_id = body.clip_id.clone();
    app.mutate(&label, move |p| {
        // Speed is structural: it changes the clip's length, so it must go
        // through the operation that keeps the timeline consistent.
        if let Some(speed) = body.patch.speed {
            ops::set_speed(p, &clip_id, speed)?;
        }
        let (_, _) = p
            .clip(&clip_id)
            .ok_or_else(|| anyhow::anyhow!("clip introuvable: {clip_id}"))?;
        for track in p.tracks.iter_mut() {
            if let Some(clip) = track.clips.iter_mut().find(|c| c.id == clip_id) {
                if let Some(v) = body.patch.opacity {
                    clip.opacity = v.clamp(0.0, 1.0);
                }
                if let Some(v) = body.patch.volume {
                    clip.volume = v.clamp(0.0, 8.0);
                }
                if let Some(v) = body.patch.fade_in {
                    clip.fade_in = v.max(0);
                }
                if let Some(v) = body.patch.fade_out {
                    clip.fade_out = v.max(0);
                }
                if let Some(v) = body.patch.transform {
                    clip.transform = v;
                }
                if let Some(v) = &body.patch.effects {
                    clip.effects = v.clone();
                }
                if let Some(v) = &body.patch.text {
                    clip.text = Some(v.clone());
                }
                break;
            }
        }
        Ok(())
    })?;
    Ok(Json(app.snapshot()))
}

#[derive(Deserialize)]
pub struct CreateText {
    pub content: String,
    pub timeline_start: Micros,
    #[serde(default)]
    pub duration: Option<Micros>,
    #[serde(default)]
    pub font_size: Option<u32>,
}

pub async fn clip_text(State(app): AppState, Json(body): Json<CreateText>) -> ApiResult<Snapshot> {
    let duration = body.duration.unwrap_or(3 * ONE_SECOND);
    let size = body.font_size.unwrap_or(72);
    app.mutate("Ajouter un texte", move |p| {
        // Text lives on its own track so it can never fight a clip for space,
        // and it sits above the media because track order is z-order.
        let track_id = p
            .tracks
            .iter()
            .find(|t| t.kind == TrackKind::Video && t.name == TEXT_TRACK_NAME)
            .map(|t| t.id.clone())
            .unwrap_or_else(|| ops::add_track(p, TrackKind::Video, Some(TEXT_TRACK_NAME.into())));
        let text = TextOverlay {
            content: body.content.clone(),
            font_family: "DejaVu Sans".into(),
            font_size: size,
            color: Rgba {
                r: 255,
                g: 255,
                b: 255,
                a: 255,
            },
            align: TextAlign::Center,
            background: Some(Rgba {
                r: 16,
                g: 14,
                b: 34,
                a: 150,
            }),
            bold: true,
            italic: false,
        };
        ops::place_text(p, &track_id, text, body.timeline_start, duration).map(|_| ())
    })?;
    Ok(Json(app.snapshot()))
}

const TEXT_TRACK_NAME: &str = "Texte";

// ------------------------------------------------------------------ tracks

#[derive(Deserialize)]
pub struct AddTrack {
    pub kind: TrackKind,
    #[serde(default)]
    pub name: Option<String>,
}

pub async fn track_add(State(app): AppState, Json(body): Json<AddTrack>) -> ApiResult<Snapshot> {
    app.mutate("Ajouter une piste", move |p| {
        ops::add_track(p, body.kind, body.name);
        Ok(())
    })?;
    Ok(Json(app.snapshot()))
}

#[derive(Deserialize)]
pub struct TrackRef {
    pub track_id: String,
}

pub async fn track_remove(State(app): AppState, Json(body): Json<TrackRef>) -> ApiResult<Snapshot> {
    app.mutate("Supprimer une piste", move |p| {
        ops::remove_track(p, &body.track_id)
    })?;
    Ok(Json(app.snapshot()))
}

#[derive(Deserialize)]
pub struct TrackPatch {
    pub track_id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub muted: Option<bool>,
    #[serde(default)]
    pub locked: Option<bool>,
    #[serde(default)]
    pub hidden: Option<bool>,
}

pub async fn track_update(
    State(app): AppState,
    Json(body): Json<TrackPatch>,
) -> ApiResult<Snapshot> {
    app.mutate("Modifier une piste", move |p| {
        let track = p
            .track_mut(&body.track_id)
            .ok_or_else(|| anyhow::anyhow!("piste introuvable"))?;
        if let Some(v) = body.name {
            track.name = v;
        }
        if let Some(v) = body.muted {
            track.muted = v;
        }
        if let Some(v) = body.locked {
            track.locked = v;
        }
        if let Some(v) = body.hidden {
            track.hidden = v;
        }
        Ok(())
    })?;
    Ok(Json(app.snapshot()))
}

// ----------------------------------------------------------------- history

pub async fn undo(State(app): AppState) -> ApiResult<Snapshot> {
    app.undo();
    Ok(Json(app.snapshot()))
}

pub async fn redo(State(app): AppState) -> ApiResult<Snapshot> {
    app.redo();
    Ok(Json(app.snapshot()))
}

// ------------------------------------------------------------------- jobs

#[derive(Deserialize)]
pub struct PreviewRequest {
    #[serde(default)]
    pub from: Option<Micros>,
    #[serde(default)]
    pub to: Option<Micros>,
    #[serde(default)]
    pub height: Option<u32>,
}

pub async fn preview(State(app): AppState, Json(body): Json<PreviewRequest>) -> ApiResult<Job> {
    let height = body.height.unwrap_or(360).clamp(120, 720);
    let from = body.from.unwrap_or(0);
    let to = body.to.unwrap_or(0);
    let id = uuid_like();
    let job = Job {
        id: id.clone(),
        kind: "preview".into(),
        status: JobStatus::Running,
        progress: 0.0,
        message: "Préparation".into(),
        output: None,
        bytes: None,
        encoder: None,
        elapsed: None,
    };
    app.register(&id, job.clone(), Arc::new(AtomicBool::new(false)));

    let app2 = app.clone();
    let id2 = id.clone();
    tokio::task::spawn_blocking(move || {
        app2.update_job(&id2, |j| j.message = "Rendu de l'aperçu".into());
        match crate::state::render_preview(&app2, from, to, height) {
            Ok(()) => {
                let bytes = std::fs::metadata(&app2.preview_path)
                    .map(|m| m.len())
                    .unwrap_or(0);
                app2.update_job(&id2, |j| {
                    j.status = JobStatus::Done;
                    j.progress = 1.0;
                    j.message = "Aperçu prêt".into();
                    j.bytes = Some(bytes);
                    // Cache-buster: the same path is rewritten on every preview.
                    j.output = Some(format!("/media/preview.mp4?v={}", now_ms()));
                });
            }
            Err(err) => app2.update_job(&id2, |j| {
                j.status = JobStatus::Failed;
                j.message = err.to_string();
            }),
        }
        app2.clear_cancel(&id2);
    });

    Ok(Json(job))
}

#[derive(Deserialize)]
pub struct ExportRequest {
    #[serde(default)]
    pub height: Option<u32>,
    #[serde(default)]
    pub container: Option<Container>,
    #[serde(default)]
    pub quality: Option<Quality>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

pub async fn export(State(app): AppState, Json(body): Json<ExportRequest>) -> ApiResult<Job> {
    let project = app.project();
    if project.duration() == 0 {
        return Err(ApiError(anyhow::anyhow!(
            "la timeline est vide : ajoutez au moins un clip"
        )));
    }
    let height = body.height.unwrap_or(1080).clamp(240, 2160);
    let container = body.container.unwrap_or(Container::Mp4);
    let quality = body.quality.unwrap_or(Quality::Standard);
    let name = export::with_container_extension(
        &body
            .name
            .unwrap_or_else(|| export::suggested_output(&project, container)),
        container,
    );
    let output = match body.path {
        Some(p) => PathBuf::from(p),
        None => app.media_dir.join(&name),
    };

    let id = uuid_like();
    let job = Job {
        id: id.clone(),
        kind: "export".into(),
        status: JobStatus::Running,
        progress: 0.0,
        message: "Encodage".into(),
        output: None,
        bytes: None,
        encoder: None,
        elapsed: None,
    };
    app.register(&id, job.clone(), Arc::new(AtomicBool::new(false)));

    let app2 = app.clone();
    let id2 = id.clone();
    tokio::task::spawn_blocking(move || {
        let result = crate::state::render_export(&app2, output, height, container, quality, &id2);
        match result {
            Ok(ExportReportLite {
                output,
                bytes,
                encoder,
                elapsed,
            }) => {
                let file = PathBuf::from(&output);
                let served = file
                    .file_name()
                    .map(|n| format!("/media/{}", n.to_string_lossy()))
                    .unwrap_or_default();
                app2.update_job(&id2, |j| {
                    j.status = JobStatus::Done;
                    j.progress = 1.0;
                    j.message = format!("Export terminé : {}", file.display());
                    j.bytes = Some(bytes);
                    j.encoder = Some(encoder);
                    j.elapsed = Some(elapsed);
                    j.output = Some(served);
                });
            }
            Err(err) => {
                let message = err.to_string();
                app2.update_job(&id2, |j| {
                    j.status = if message.contains("annulé") {
                        JobStatus::Cancelled
                    } else {
                        JobStatus::Failed
                    };
                    j.message = message;
                });
            }
        }
        app2.clear_cancel(&id2);
    });

    Ok(Json(job))
}

/// Serve an imported asset's bytes.
///
/// Range requests are mandatory here: a browser video element seeks with them,
/// and without range support scrubbing a large file either stalls or replays
/// from the start. `ServeFile` handles ranges, conditional requests and MIME
/// types, so the service only has to resolve the asset id to a path.
pub async fn asset_media(
    State(app): AppState,
    Path(id): Path<String>,
    request: axum::extract::Request,
) -> Response {
    let Some(asset) = app.project().asset(&id).cloned() else {
        return (StatusCode::NOT_FOUND, "média introuvable").into_response();
    };
    let path = PathBuf::from(&asset.path);
    if !path.is_file() {
        return (StatusCode::NOT_FOUND, "fichier absent").into_response();
    }
    match tower::ServiceExt::oneshot(ServeFile::new(&path), request).await {
        Ok(response) => response.into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("lecture impossible : {error}"),
        )
            .into_response(),
    }
}

pub async fn job(State(app): AppState, Path(id): Path<String>) -> ApiResult<Job> {
    app.job(&id)
        .map(Json)
        .ok_or_else(|| ApiError(anyhow::anyhow!("tâche inconnue")))
}

pub async fn cancel_job(State(app): AppState, Path(id): Path<String>) -> ApiResult<Ok_> {
    app.cancel(&id);
    Ok(Json(Ok_ { ok: true }))
}

fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

fn uuid_like() -> String {
    uuid::Uuid::new_v4().to_string()
}
