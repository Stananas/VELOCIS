//! Shared state: the open project, its undo history, and render jobs.

use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use velocis_engine::export::{self, Container, Encoder, ExportSettings, Quality};
use velocis_engine::model::*;
use velocis_engine::ops::Editor;
use velocis_engine::render::{self, RenderRange};
use velocis_engine::{probe, thumbs};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Running,
    Done,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize)]
pub struct Job {
    pub id: String,
    pub kind: String,
    pub status: JobStatus,
    /// 0.0..=1.0
    pub progress: f64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encoder: Option<Encoder>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elapsed: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionInfo {
    pub can_undo: bool,
    pub can_redo: bool,
    pub undo_label: Option<String>,
    pub redo_label: Option<String>,
    pub duration: Micros,
    pub encoder: Encoder,
    pub hardware_accelerated: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    pub project: Project,
    pub session: SessionInfo,
}

pub struct App {
    pub editor: Mutex<Editor>,
    pub jobs: Mutex<HashMap<String, Job>>,
    pub cancels: Mutex<HashMap<String, Arc<AtomicBool>>>,
    /// Where the project lives on disk, once it has been saved or opened.
    pub project_path: Mutex<Option<PathBuf>>,
    /// Root for generated media (thumbnails, previews, exports).
    pub media_dir: PathBuf,
    /// A single preview file is reused: previews are cheap and disposable.
    pub preview_path: PathBuf,
}

impl App {
    pub fn new(media_dir: PathBuf) -> Self {
        std::fs::create_dir_all(&media_dir).ok();
        let preview_path = media_dir.join("preview.mp4");
        let autosave = media_dir.join("autosave.json");

        // Resume the previous session if there is one. Editing is iterative, and
        // losing the timeline on every restart would make the tool unusable.
        let project = std::fs::read_to_string(&autosave)
            .ok()
            .and_then(|text| serde_json::from_str::<Project>(&text).ok())
            .unwrap_or_else(|| {
                Project::new(
                    "Projet sans titre",
                    Canvas {
                        width: 1920,
                        height: 1080,
                        fps: 30.0,
                        background: Rgba {
                            r: 12,
                            g: 12,
                            b: 18,
                            a: 255,
                        },
                    },
                )
            });

        Self {
            editor: Mutex::new(Editor::new(project)),
            jobs: Mutex::new(HashMap::new()),
            cancels: Mutex::new(HashMap::new()),
            project_path: Mutex::new(None),
            media_dir,
            preview_path,
        }
    }

    fn autosave_path(&self) -> PathBuf {
        self.media_dir.join("autosave.json")
    }

    /// Write the project to disk. Called after every change: the file is a few
    /// kilobytes, and an editor that can lose your work is not worth using.
    fn persist(&self, project: &Project) {
        if let Ok(json) = serde_json::to_string_pretty(project) {
            let _ = std::fs::write(self.autosave_path(), json);
        }
    }

    /// Run a mutation through the undo-recording editor.
    pub fn mutate<T>(
        &self,
        label: &str,
        f: impl FnOnce(&mut Project) -> anyhow::Result<T>,
    ) -> anyhow::Result<T> {
        let mut editor = self.editor.lock().unwrap();
        let result = editor.apply(label, f)?;
        self.persist(&editor.project);
        Ok(result)
    }

    pub fn undo(&self) -> bool {
        let mut editor = self.editor.lock().unwrap();
        let changed = editor.undo();
        if changed {
            self.persist(&editor.project);
        }
        changed
    }

    pub fn redo(&self) -> bool {
        let mut editor = self.editor.lock().unwrap();
        let changed = editor.redo();
        if changed {
            self.persist(&editor.project);
        }
        changed
    }

    pub fn snapshot(&self) -> Snapshot {
        let editor = self.editor.lock().unwrap();
        Snapshot {
            session: session_info(&editor),
            project: editor.project.clone(),
        }
    }

    pub fn project(&self) -> Project {
        self.editor.lock().unwrap().project.clone()
    }

    pub fn register(&self, id: &str, job: Job, cancel: Arc<AtomicBool>) {
        self.jobs.lock().unwrap().insert(id.to_string(), job);
        self.cancels.lock().unwrap().insert(id.to_string(), cancel);
    }

    pub fn update_job(&self, id: &str, f: impl FnOnce(&mut Job)) {
        if let Some(job) = self.jobs.lock().unwrap().get_mut(id) {
            f(job);
        }
    }

    pub fn job(&self, id: &str) -> Option<Job> {
        self.jobs.lock().unwrap().get(id).cloned()
    }

    pub fn cancel(&self, id: &str) {
        if let Some(flag) = self.cancels.lock().unwrap().get(id) {
            flag.store(true, Ordering::Relaxed);
        }
    }

    pub fn clear_cancel(&self, id: &str) {
        self.cancels.lock().unwrap().remove(id);
    }
}

pub fn session_info(editor: &Editor) -> SessionInfo {
    let caps = export::capabilities();
    SessionInfo {
        can_undo: editor.can_undo(),
        can_redo: editor.can_redo(),
        undo_label: editor.undo_label().map(|s| s.to_string()),
        redo_label: editor.redo_label().map(|s| s.to_string()),
        duration: editor.project.duration(),
        encoder: caps.encoder,
        hardware_accelerated: caps.hardware_accelerated,
    }
}

/// Import a file: probe it, then build the derivatives the timeline needs.
pub fn import_file(app: &App, path: &Path) -> anyhow::Result<MediaAsset> {
    let mut asset = probe::probe(path)?;
    let stem = format!("{:016x}", hash_string(&asset.path));
    match asset.kind {
        AssetKind::Audio => {
            let peaks = thumbs::extract_waveform(path, asset.duration, 1200)?;
            asset.waveform = Some(peaks);
        }
        _ => {
            let thumb = app.media_dir.join(format!("thumb_{stem}.jpg"));
            thumbs::write_thumbnail(path, (asset.duration / 8).max(0), 320, &thumb)?;
            asset.thumbnail_path = Some(thumb.to_string_lossy().into_owned());
            let peaks = thumbs::extract_waveform(path, asset.duration, 1200)?;
            if !peaks.peaks.is_empty() {
                asset.waveform = Some(peaks);
            }
        }
    }
    Ok(asset)
}

fn hash_string(value: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in value.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    hash
}

/// Render the project into the shared preview file, at a cheap resolution.
pub fn render_preview(app: &App, from: Micros, to: Micros, height: u32) -> anyhow::Result<()> {
    let project = app.project();
    let range = if to > from {
        RenderRange { from, to }
    } else {
        RenderRange::whole(&project)
    };
    let aspect = project.canvas.width as f64 / project.canvas.height.max(1) as f64;
    let width = ((height as f64 * aspect) / 2.0).round() as u32 * 2;
    let settings = ExportSettings::new(
        app.preview_path.clone(),
        width.max(2),
        height,
        project.canvas.fps.min(30.0),
        Container::Mp4,
        Quality::Draft,
    );
    let plan = render::build_plan_with(&project, &range, &settings.target);
    let _ = std::fs::remove_file(&app.preview_path);
    export::run(&plan, &settings, &mut |_| true)?;
    Ok(())
}

/// Render the project at full quality to `output`.
pub fn render_export(
    app: &App,
    output: PathBuf,
    height: u32,
    container: Container,
    quality: Quality,
    job_id: &str,
) -> anyhow::Result<ExportReportLite> {
    let project = app.project();
    let aspect = project.canvas.width as f64 / project.canvas.height.max(1) as f64;
    let width = ((height as f64 * aspect) / 2.0).round() as u32 * 2;
    let settings = ExportSettings::new(
        output,
        width.max(2),
        height,
        project.canvas.fps,
        container,
        quality,
    );
    let range = RenderRange::whole(&project);
    let plan = render::build_plan_with(&project, &range, &settings.target);

    let cancel = app
        .cancels
        .lock()
        .unwrap()
        .get(job_id)
        .cloned()
        .unwrap_or_else(|| Arc::new(AtomicBool::new(false)));

    let report = export::run(&plan, &settings, &mut |p| {
        app.update_job(job_id, |job| job.progress = p.ratio);
        !cancel.load(Ordering::Relaxed)
    })?;

    Ok(ExportReportLite {
        output: report.output,
        bytes: report.bytes,
        encoder: report.encoder,
        elapsed: report.elapsed_register,
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct ExportReportLite {
    pub output: String,
    pub bytes: u64,
    pub encoder: Encoder,
    pub elapsed: f64,
}
