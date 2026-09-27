//! The `librepcb-mcp` binary (see the crate documentation).

use std::process::ExitCode;

use clap::Parser;
use librepcb_mcp::{LibrePcbMcp, Session};
use rmcp::ServiceExt;

/// MCP server for LibrePCB projects and libraries.
#[derive(Debug, Parser)]
#[command(name = "librepcb-mcp", version, about)]
struct Args {
    /// Workspace directory to open (created if it does not exist).
    #[arg(long)]
    workspace: Option<String>,
    /// Project (*.lpp file or directory) to open at startup.
    #[arg(long)]
    project: Option<String>,
    /// Serve streamable HTTP on this address (e.g. 127.0.0.1:8080, path
    /// /mcp) instead of stdio (needs the cargo feature `http`).
    #[arg(long)]
    http: Option<std::net::SocketAddr>,
}

fn setup(args: &Args) -> Result<Session, String> {
    let mut session = Session::new();
    if let Some(ws) = &args.workspace {
        session
            .open_workspace(ws, true)
            .map_err(|e| format!("cannot open workspace {ws}: {e}"))?;
    }
    if let Some(p) = &args.project {
        session
            .open_project(p)
            .map_err(|e| format!("cannot open project {p}: {e}"))?;
    }
    Ok(session)
}

#[tokio::main]
async fn main() -> ExitCode {
    // Logs go to stderr; stdout is the MCP transport.
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn"))
        .target(env_logger::Target::Stderr)
        .init();
    let args = Args::parse();
    let session = match tokio::task::spawn_blocking({
        let args = Args {
            workspace: args.workspace.clone(),
            project: args.project.clone(),
            http: args.http,
        };
        move || setup(&args)
    })
    .await
    {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            eprintln!("librepcb-mcp: {e}");
            return ExitCode::FAILURE;
        }
        Err(e) => {
            eprintln!("librepcb-mcp: {e}");
            return ExitCode::FAILURE;
        }
    };
    let server = LibrePcbMcp::new(session);
    let result = match args.http {
        Some(addr) => serve_http(server, addr).await,
        None => serve_stdio(server).await,
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("librepcb-mcp: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn serve_stdio(server: LibrePcbMcp) -> Result<(), String> {
    let running = server
        .serve(rmcp::transport::stdio())
        .await
        .map_err(|e| e.to_string())?;
    running.waiting().await.map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(feature = "http")]
async fn serve_http(server: LibrePcbMcp, addr: std::net::SocketAddr) -> Result<(), String> {
    use rmcp::transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
    };
    let session = std::sync::Arc::clone(server.session());
    let service: StreamableHttpService<LibrePcbMcp, LocalSessionManager> =
        StreamableHttpService::new(
            move || {
                Ok(LibrePcbMcp::with_shared_session(std::sync::Arc::clone(
                    &session,
                )))
            },
            Default::default(),
            StreamableHttpServerConfig::default(),
        );
    let router = axum::Router::new().nest_service("/mcp", service);
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| e.to_string())?;
    eprintln!("librepcb-mcp: serving streamable HTTP on http://{addr}/mcp");
    axum::serve(listener, router)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .map_err(|e| e.to_string())
}

#[cfg(not(feature = "http"))]
async fn serve_http(_server: LibrePcbMcp, _addr: std::net::SocketAddr) -> Result<(), String> {
    Err("this build has no HTTP transport (build with --features http)".to_owned())
}
