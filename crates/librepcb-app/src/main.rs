//! The `librepcb` binary: command line handling and the event loop.
//!
//! Port of apps/librepcb/main.cpp.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, ValueEnum};
use librepcb_app::screenshot;
use librepcb_app::startup;
use librepcb_app::ui;
use librepcb_app::{App, InitialTab};
use slint::ComponentHandle;

/// Tab to show (screenshot mode or after opening a project).
#[derive(Debug, Clone, Copy, ValueEnum)]
enum TabArg {
    Home,
    Schematic,
    Board,
}

/// Side panel page to show.
#[derive(Debug, Clone, Copy, ValueEnum)]
enum PanelArg {
    Home,
    Documents,
    Layers,
    About,
    None,
}

/// LibrePCB, the electronic design automation software.
#[derive(Debug, Parser)]
#[command(name = "librepcb", version)]
struct Args {
    /// Workspace directory (default: like upstream LibrePCB).
    #[arg(long)]
    workspace: Option<PathBuf>,
    /// Project file (*.lpp) to open.
    #[arg(long)]
    project: Vec<PathBuf>,
    /// Files to open (projects).
    files: Vec<PathBuf>,
    /// Language (e.g. "de"; default: workspace setting or system).
    #[arg(long)]
    language: Option<String>,
    /// Tab to show after opening the project.
    #[arg(long, value_enum)]
    tab: Option<TabArg>,
    /// Side panel page to show.
    #[arg(long, value_enum)]
    panel: Option<PanelArg>,
    /// Render the window headless into this PNG file and exit.
    #[arg(long)]
    screenshot: Option<PathBuf>,
    /// Window size for --screenshot, e.g. 1400x900.
    #[arg(long, default_value = "1400x900")]
    size: String,
}

fn parse_size(s: &str) -> Option<(u32, u32)> {
    let (w, h) = s.split_once('x')?;
    let (w, h) = (w.trim().parse().ok()?, h.trim().parse().ok()?);
    (w >= 200 && h >= 200 && w <= 8192 && h <= 8192).then_some((w, h))
}

fn main() -> ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let args = Args::parse();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &Args) -> Result<(), Box<dyn std::error::Error>> {
    let headless = match &args.screenshot {
        Some(_) => {
            let (w, h) = parse_size(&args.size).ok_or("invalid --size, expected WxH")?;
            Some(screenshot::install_platform(w, h)?)
        }
        None => None,
    };

    let ws_path = startup::workspace_path(args.workspace.as_deref())
        .ok_or("could not determine the workspace directory, pass --workspace")?;
    log::info!("Workspace: {}", ws_path.display());
    let workspace = startup::open_workspace(&ws_path)?;
    let language = startup::select_language(args.language.as_deref(), &workspace);

    let window = ui::AppWindow::new()?;
    // Slint requires a component to exist before selecting the language.
    if let Err(e) = slint::select_bundled_translation(language) {
        log::warn!("Failed to select the UI language {language}: {e}");
    }
    let app = App::new(window, workspace);

    let mut opened = false;
    for file in args.project.iter().chain(&args.files) {
        opened |= app.open_project(file).is_some();
    }
    if let Some(tab) = args.tab {
        app.show_tab(match tab {
            TabArg::Home => InitialTab::Home,
            TabArg::Schematic => InitialTab::Schematic,
            TabArg::Board => InitialTab::Board,
        });
    }
    if let Some(panel) = args.panel {
        app.show_panel(match panel {
            PanelArg::Home => ui::PanelPage::Home,
            PanelArg::Documents => ui::PanelPage::Documents,
            PanelArg::Layers => ui::PanelPage::Layers,
            PanelArg::About => ui::PanelPage::About,
            PanelArg::None => ui::PanelPage::None,
        });
    }
    if !opened && (!args.project.is_empty() || !args.files.is_empty()) {
        log::warn!("No project could be opened.");
    }

    match (headless, &args.screenshot) {
        (Some(headless), Some(path)) => {
            app.window().show()?;
            headless.save_png(path)?;
            log::info!("Screenshot written to {}", path.display());
            Ok(())
        }
        _ => {
            app.window().run()?;
            Ok(())
        }
    }
}
