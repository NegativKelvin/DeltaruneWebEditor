//! dre-server: local backend for the Deltarune web mod editor.

use dre_server::{api, guard, utmt};

use std::net::SocketAddr;
use std::path::PathBuf;

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
    // A saved path goes stale when the release folder moves: find the CLI again.
    if !settings.utmt_cli.as_ref().is_some_and(|c| c.is_file()) {
        settings.utmt_cli = utmt::locate_cli();
    }
    let web = near_exe(args.web);
    tracing::info!(?settings, workspace = %workspace.display(), "starting");
    std::fs::write(workspace.join("settings.json"), serde_json::to_vec_pretty(&settings)?)?;

    let state = api::AppState::new(workspace, settings);
    let app = api::router(state)
        .fallback_service(ServeDir::new(&web))
        .layer(axum::middleware::from_fn(guard::same_site_writes))
        .layer(axum::middleware::from_fn(guard::local_host_only))
        .layer(TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind(args.listen).await?;
    tracing::info!("listening on http://{}", args.listen);
    axum::serve(listener, app).await
}

/// Uses `<server folder>/<path>` when a relative path is not in the current directory (release zip started from elsewhere).
fn near_exe(path: PathBuf) -> PathBuf {
    if path.is_relative()
        && !path.exists()
        && let Some(p) = utmt::exe_dir().map(|d| d.join(&path)).filter(|p| p.exists())
    {
        return p;
    }
    path
}
