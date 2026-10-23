//! `velocisd` — the local editing service.
//!
//! The UI is a plain web page served from the same origin, so there is no CORS
//! surface and no bundler in the loop. All editing decisions live in `velocis_engine`;
//! this layer only translates HTTP into engine calls, keeps the undo history,
//! and runs renders as cancellable jobs.
//!
//! The service binds to loopback only. It exposes filesystem paths chosen by the
//! user, so it must never be reachable from the network.

mod api;
mod state;

use anyhow::Result;
use axum::extract::DefaultBodyLimit;
use axum::routing::{get, post};
use axum::Router;
use state::App;
use std::path::PathBuf;
use std::sync::Arc;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;

/// The UI ships as static files next to the sources, so a debug build runs
/// straight from the working tree with no build step.
fn ui_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("VELOCIS_UI_DIR") {
        return PathBuf::from(dir);
    }
    let candidate = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("ui");
    candidate.canonicalize().unwrap_or(candidate)
}

/// Where the service keeps projects, uploaded media and renders.
///
/// A real application must not scatter user data in the system temporary
/// directory: `/tmp` is wiped on reboot and is world-readable on many systems.
/// The XDG data directory is the right home, with an explicit override for
/// portable installs and tests.
pub fn data_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("VELOCIS_DATA_DIR") {
        return PathBuf::from(dir);
    }
    let base = std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .ok()
        .filter(|p| p.is_absolute())
        .or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(|home| PathBuf::from(home).join(".local").join("share"))
        })
        .unwrap_or_else(std::env::temp_dir);
    base.join("velocis")
}

fn media_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("VELOCIS_MEDIA_DIR") {
        return PathBuf::from(dir);
    }
    data_dir().join("media")
}

/// Open the interface in the user's browser.
///
/// A desktop shell would be the next step, but until then this is what makes
/// `velocisd` feel like an application rather than a server.
fn open_browser(url: &str) {
    let commands: [(&str, Vec<&str>); 3] = [
        ("xdg-open", vec![url]),
        ("open", vec![url]),
        ("cmd", vec!["/C", "start", url]),
    ];
    for (program, args) in commands {
        if std::process::Command::new(program)
            .args(&args)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .is_ok()
        {
            return;
        }
    }
    eprintln!("impossible d'ouvrir le navigateur : rendez-vous sur {url}");
}

#[tokio::main]
async fn main() -> Result<()> {
    let app = Arc::new(App::new(media_dir()));
    let ui = ui_dir();
    let should_open = std::env::args().any(|arg| arg == "--open" || arg == "-o");

    let router = Router::new()
        .route("/api/state", get(api::state))
        .route("/api/capabilities", get(api::capabilities))
        .route("/api/project/new", post(api::project_new))
        .route("/api/project/rename", post(api::project_rename))
        .route("/api/project/canvas", post(api::project_canvas))
        .route("/api/project/save", post(api::project_save))
        .route("/api/project/open", post(api::project_open))
        .route("/api/import", post(api::import))
        .route(
            "/api/upload",
            // Uploads are real media files: the default 2 MB body limit would
            // reject every video a user actually wants to edit.
            post(api::upload).layer(DefaultBodyLimit::max(8 * 1024 * 1024 * 1024)),
        )
        .route("/api/media/{id}", get(api::asset_media))
        .route("/api/clip/append", post(api::clip_append))
        .route("/api/clip/place", post(api::clip_place))
        .route("/api/clip/move", post(api::clip_move))
        .route("/api/clip/trim", post(api::clip_trim))
        .route("/api/clip/split", post(api::clip_split))
        .route("/api/clip/split-all", post(api::clip_split_all))
        .route("/api/clip/delete", post(api::clip_delete))
        .route("/api/clip/duplicate", post(api::clip_duplicate))
        .route("/api/clip/update", post(api::clip_update))
        .route("/api/clip/text", post(api::clip_text))
        .route("/api/track/add", post(api::track_add))
        .route("/api/track/remove", post(api::track_remove))
        .route("/api/track/update", post(api::track_update))
        .route("/api/history/undo", post(api::undo))
        .route("/api/history/redo", post(api::redo))
        .route("/api/preview", post(api::preview))
        .route("/api/export", post(api::export))
        .route("/api/proxy", post(api::proxy_segment))
        .route("/api/prepare", post(api::prepare))
        .route("/api/jobs/{id}", get(api::job))
        .route("/api/jobs/{id}/cancel", post(api::cancel_job))
        .nest_service("/media", ServeDir::new(app.media_dir.clone()))
        .fallback_service(
            ServeDir::new(&ui).not_found_service(ServeFile::new(ui.join("index.html"))),
        )
        .layer(TraceLayer::new_for_http())
        .with_state(app.clone());

    let port: u16 = std::env::var("VELOCIS_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8730);
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let url = format!("http://{addr}");
    println!("Velocis écoute sur {url}");
    println!("interface : {}", ui.display());
    println!("données   : {}", app.media_dir.display());
    if should_open {
        open_browser(&url);
    }
    axum::serve(listener, router).await?;
    Ok(())
}
