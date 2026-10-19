//! End-to-end smoke test against real media and real ffmpeg.
//!
//! Run with:
//!   cargo run --release --example smoke -- <fichier1.mp4> [fichier2.mp4]
//!
//! It exercises the whole engine: probe -> assets -> timeline -> plan ->
//! preview render -> full export, plus thumbnail and waveform generation. The
//! point is to catch filter-graph mistakes that unit tests cannot see, because
//! only ffmpeg can reject a graph.

use std::path::PathBuf;
use velocis_engine::export::{self, Container, ExportSettings};
use velocis_engine::model::*;
use velocis_engine::render::{self, RenderRange};
use velocis_engine::{ops, probe, thumbs};

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        anyhow::bail!("usage: smoke <fichier média> [fichier média 2]");
    }

    let out_dir = PathBuf::from("/tmp/opencode/smoke");
    std::fs::create_dir_all(&out_dir)?;

    let mut project = Project::new(
        "Démo du moteur",
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
    );

    // ---- import ---------------------------------------------------------
    for (i, path) in args.iter().enumerate() {
        let asset = probe::probe(&PathBuf::from(path))?;
        println!(
            "asset {}: {} | {:?} | {:.2}s | {}x{} @ {:.3} fps | v={:?} a={:?}",
            i + 1,
            asset.name,
            asset.kind,
            asset.duration as f64 / 1e6,
            asset.width,
            asset.height,
            asset.fps,
            asset.video_codec,
            asset.audio_codec
        );

        // Derivatives: poster frame + audio peaks.
        let thumb = out_dir.join(format!("thumb{i}.jpg"));
        if asset.kind != AssetKind::Audio {
            thumbs::write_thumbnail(PathBuf::from(path).as_path(), 0, 320, &thumb)
                .map_err(|e| anyhow::anyhow!("vignette: {e}"))?;
            let size = std::fs::metadata(&thumb).map(|m| m.len()).unwrap_or(0);
            println!("  vignette -> {} ({size} octets)", thumb.display());
        }
        let peaks = thumbs::extract_waveform(PathBuf::from(path).as_path(), asset.duration, 96)?;
        println!("  forme d'onde -> {} buckets", peaks.peaks.len());

        ops::import_asset(&mut project, asset);
    }

    // ---- montage --------------------------------------------------------
    let video_track = project.tracks[0].id.clone();
    let first = project.assets[0].clone();
    let clip = ops::append_clip(
        &mut project,
        &video_track,
        &first.id,
        Some(TimeRange::new(0, (2.5 * ONE_SECOND as f64) as Micros)),
    )?;
    println!("clip vidéo: {clip}");

    // A second clip on a track above, scaled and flipped, to prove compositing.
    if project.assets.len() > 1 {
        let overlay_track = ops::add_track(&mut project, TrackKind::Video, None);
        let second = project.assets[1].id.clone();
        let top = ops::place_clip(
            &mut project,
            &overlay_track,
            &second,
            TimeRange::new(0, ONE_SECOND),
            500_000,
        )?;
        if let Some(track) = project.track_mut(&overlay_track) {
            if let Some(c) = track.clip(&top) {
                let id = c.id.clone();
                if let Some(c) = track.clips.iter_mut().find(|c| c.id == id) {
                    c.transform.scale = 0.42;
                    c.transform.x = -420.0;
                    c.transform.y = 220.0;
                    c.transform.rotation_deg = -4.0;
                    c.transform.mirror_x = true;
                    c.opacity = 0.92;
                    c.fade_in = 400_000;
                    c.fade_out = 400_000;
                    c.volume = 0.35;
                    c.effects.push(Effect::Saturation { amount: 0.25 });
                    c.effects.push(Effect::Vignette { amount: 0.7 });
                }
            }
        }
    }

    // A title, to exercise the synthetic-source path.
    if let Some(track) = project.track_mut(&video_track) {
        let mut title = Clip::new(
            "synthetique",
            TimeRange::new(0, 2 * ONE_SECOND),
            TimeRange::new(200_000, 2 * ONE_SECOND),
        );
        title.text = Some(TextOverlay {
            content: "Salut : le moteur tourne, 100% Rust".into(),
            font_family: "DejaVu Sans".into(),
            font_size: 84,
            color: Rgba {
                r: 255,
                g: 255,
                b: 255,
                a: 255,
            },
            align: TextAlign::Center,
            background: Some(Rgba {
                r: 20,
                g: 16,
                b: 40,
                a: 170,
            }),
            bold: true,
            italic: false,
        });
        title.fade_in = 300_000;
        title.fade_out = 300_000;
        track.clips.push(title);
        track.normalize();
    }

    println!(
        "projet: {} pistes, durée {:.2}s",
        project.tracks.len(),
        project.duration() as f64 / 1e6
    );

    // ---- plan + preview -------------------------------------------------
    let range = RenderRange::whole(&project);
    let preview = out_dir.join("preview.mp4");
    let preview_settings = ExportSettings::new(
        preview.clone(),
        640,
        360,
        30.0,
        Container::Mp4,
        export::Quality::Draft,
    );
    let preview_plan = render::build_plan_with(&project, &range, &preview_settings.target);
    println!(
        "plan preview: {} inputs, {} avertissements, {} octets de graphe, encodeur {:?}",
        preview_plan.inputs.len(),
        preview_plan.warnings.len(),
        preview_plan.filter_complex.len(),
        preview_settings.encoder
    );
    for w in &preview_plan.warnings {
        println!("  avertissement: {w}");
    }

    let report = export::run(&preview_plan, &preview_settings, &mut |p| {
        if (p.ratio * 100.0) as i32 % 25 == 0 {
            print!("\r  preview {:3.0}%", p.ratio * 100.0);
        }
        true
    })?;
    println!(
        "\n  preview ok: {} ({:.1} Mo, encodeur {:?})",
        preview.display(),
        report.bytes as f64 / 1e6,
        report.encoder
    );

    // ---- export ---------------------------------------------------------
    let output = out_dir.join(export::suggested_output(&project, Container::Mp4));
    let settings = ExportSettings::new(
        output.clone(),
        project.canvas.width,
        project.canvas.height,
        30.0,
        Container::Mp4,
        export::Quality::Standard,
    );
    // Build with the target the chosen encoder needs (hardware upload lives there).
    let full_plan = render::build_plan_with(&project, &range, &settings.target);
    let report = export::run(&full_plan, &settings, &mut |_| true)?;
    println!(
        "  export ok: {} ({:.1} Mo, {:.1}s, encodeur {:?})",
        output.display(),
        report.bytes as f64 / 1e6,
        report.elapsed_register,
        report.encoder
    );

    // Write the graph out: it is the artefact that makes a render reproducible.
    let graph_path = out_dir.join("filtergraph.txt");
    std::fs::write(&graph_path, full_plan.pretty())?;
    println!("  graphe: {}", graph_path.display());

    let project_path = out_dir.join("project.json");
    std::fs::write(&project_path, serde_json::to_string_pretty(&project)?)?;
    println!("  projet: {}", project_path.display());
    Ok(())
}
