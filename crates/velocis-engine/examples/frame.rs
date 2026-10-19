//! Render a single frame of a saved project, for inspection.
//!
//!   cargo run --example frame -- <projet.json> <sortie.png> <temps_s> [piste_à_masquer]
//!
//! Two runs of this, one with a track hidden, produce an exact picture of what a
//! single track contributes — which is how compositing geometry gets verified
//! without eyeballing screenshots.

use std::path::PathBuf;
use velocis_engine::export::{Container, ExportSettings, Quality};
use velocis_engine::model::*;
use velocis_engine::render::{self, RenderRange, RenderTarget};

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 3 {
        anyhow::bail!("usage: frame <projet.json> <sortie.png> <temps_s> [piste_à_masquer]");
    }
    let project_path = PathBuf::from(&args[0]);
    let output = PathBuf::from(&args[1]);
    let at: f64 = args[2].parse()?;
    let hide: Option<usize> = args.get(3).and_then(|v| v.parse().ok());

    let mut project: Project = serde_json::from_str(&std::fs::read_to_string(&project_path)?)?;
    if let Some(index) = hide {
        if let Some(track) = project.tracks.get_mut(index) {
            track.hidden = true;
        }
    }

    let at_micros = (at * ONE_SECOND as f64).round() as Micros;
    let range = RenderRange {
        from: at_micros,
        to: at_micros + 40_000, // 40 ms: long enough for exactly one frame
    };
    let settings = ExportSettings::new(
        output.clone(),
        project.canvas.width,
        project.canvas.height,
        project.canvas.fps,
        Container::Mp4,
        Quality::High,
    );
    let mut target: RenderTarget = settings.target.clone();
    target.fps = project.canvas.fps;
    // This tool only ever writes a still: keep the graph in software so no
    // hardware device is needed.
    target.finish = None;
    let plan = render::build_plan_with(&project, &range, &target);

    // A single PNG: swap the container for an image writer.
    let mut cmd = std::process::Command::new(velocis_engine::probe::ffmpeg_bin());
    cmd.args(["-hide_banner", "-loglevel", "error", "-y"]);
    for input in &plan.inputs {
        cmd.arg("-i").arg(input);
    }
    cmd.arg("-filter_complex").arg(&plan.filter_complex);
    if let Some(v) = &plan.video_label {
        cmd.arg("-map").arg(v);
    }
    cmd.args(["-frames:v", "1"]).arg(&output);
    // The graph always defines an audio output; ffmpeg refuses to leave a
    // filtergraph output unconsumed, so send it to a null sink.
    if let Some(a) = &plan.audio_label {
        cmd.arg("-map").arg(a).args(["-f", "null", "-"]);
    }

    for file in &plan.text_files {
        if let Some(parent) = file.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&file.path, &file.content)?;
    }

    let out = cmd.output()?;
    if !out.status.success() {
        anyhow::bail!(
            "ffmpeg: {}\ngraphe:\n{}",
            String::from_utf8_lossy(&out.stderr),
            plan.pretty()
        );
    }
    println!("écrit {}", output.display());
    Ok(())
}
