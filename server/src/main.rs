//! dre-server: local backend for the Deltarune web mod editor.

use dre_server::{api, utmt};

use std::net::SocketAddr;
use std::path::PathBuf;

use axum::response::IntoResponse;
use clap::Parser;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;

#[derive(Parser, Debug)]
#[command(version, about)]
struct Args {
    /// Deltarune install directory (holds data.win and chapterN_* dirs).
    #[arg(long, env = "DRE_GAME_DIR")]
    game: Option<PathBuf>,
    /// UndertaleModCli executable (UTMT 0.9.2.0 or later). Found automatically if omitted.
    #[arg(long, env = "DRE_UTMT_CLI")]
    utmt: Option<PathBuf>,
    /// Directory for exported assets, mods, and builds.
    #[arg(long, env = "DRE_WORKSPACE", default_value = "workspace")]
    workspace: PathBuf,
    /// Directory of the webpage to serve at `/`.
    #[arg(long, env = "DRE_WEB_DIR", default_value = "web")]
    web: PathBuf,
    /// Address to listen on. Keep it on localhost: the API reads and writes local files.
    #[arg(long, env = "DRE_LISTEN", default_value = "127.0.0.1:7878")]
    listen: SocketAddr,
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,tower_http=info".into()))
        .init();
    let args = Args::parse();

    std::fs::create_dir_all(&args.workspace)?;
    let workspace = std::fs::canonicalize(&args.workspace)?;
    let mut settings = api::AppState::load_saved_settings(&workspace);
    if args.game.is_some() {
        settings.game_dir = args.game;
    }
    if args.utmt.is_some() {
        settings.utmt_cli = args.utmt;
    }
    if settings.utmt_cli.is_none() {
        settings.utmt_cli = utmt::locate_cli();
    }
    tracing::info!(?settings, workspace = %workspace.display(), "starting");
    std::fs::write(workspace.join("settings.json"), serde_json::to_vec_pretty(&settings)?)?;

    let state = api::AppState::new(workspace, settings);
    let app = api::router(state)
        .fallback_service(ServeDir::new(&args.web))
        .layer(axum::middleware::from_fn(local_host_only))
        .layer(TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind(args.listen).await?;
    tracing::info!("listening on http://{}", args.listen);
    axum::serve(listener, app).await
}

/// Blocks DNS-rebinding: only accept requests addressed to a loopback host name.
async fn local_host_only(req: axum::extract::Request, next: axum::middleware::Next) -> axum::response::Response {
    let host = req
        .headers()
        .get(axum::http::header::HOST)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    let name = host.rsplit_once(':').map_or(host, |(n, _)| n);
    if matches!(name, "localhost" | "127.0.0.1" | "[::1]") {
        next.run(req).await
    } else {
        (axum::http::StatusCode::FORBIDDEN, "host not allowed").into_response()
    }
}
