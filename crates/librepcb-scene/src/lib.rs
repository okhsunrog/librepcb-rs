//! Scene builders turning schematic pages and boards of a loaded
//! [`Project`](librepcb_core::project::Project) into
//! [`librepcb_canvas`] scenes, and a headless PNG rendering API (used by
//! the MCP `render` tool and later by the viewer).
//!
//! Ports the drawing semantics of upstream's painters
//! (libs/librepcb/core/project/schematic/schematicpainter.cpp,
//! libs/librepcb/core/project/board/boardpainter.cpp,
//! libs/librepcb/core/export/graphicspainter.cpp), the graphics items of
//! the editor (libs/librepcb/editor/project/{schematic,board}/graphicsitems)
//! and the default color schemes
//! (libs/librepcb/core/workspace/basecolorscheme.cpp).
//!
//! - [`SchematicScene`] and [`BoardScene`] build a [`Scene`] and map its
//!   items back to model objects ([`SchematicObject`], [`BoardObject`]) for
//!   hit testing. Canvas layers are color roles (schematic) or board layers
//!   and their derived layers ([`BoardSceneLayer`]).
//! - [`SymbolScene`] and [`FootprintScene`] build scenes of library
//!   elements.
//! - [`render_schematic_png()`] and [`render_board_png()`] render a page or
//!   a board side into a PNG, fitted to the content ([`RenderOptions`]).
//! - [`export`]: the graphics export (PDF, SVG, images) of upstream's
//!   `GraphicsExport` with the painters of schematics, boards, symbols and
//!   footprints, and the [`export::ProjectGraphicsExporter`] of the output
//!   job runner.
//!
//! The canvas crate stays generic; everything LibrePCB specific about
//! drawing lives here. Scenes are rebuilt from scratch; incremental updates
//! from the change journal are left to the viewer (M2).
//!
//! [`Scene`]: librepcb_canvas::Scene

mod attributes;
mod board;
mod colors;
mod error;
pub mod export;
mod footprint;
mod render;
mod schematic;
mod shapes;
mod symbol;
mod text;

pub use board::{BoardObject, BoardScene, BoardSceneLayer, BoardSide};
pub use colors::ColorScheme;
pub use error::{Error, Result};
pub use footprint::FootprintScene;
pub use render::{
    MAX_IMAGE_SIZE, RenderOptions, RenderSize, RgbaImage, render_board_png, render_board_scene,
    render_scene, render_schematic_png, render_schematic_scene, visible_bounds,
};
pub use schematic::{SchematicObject, SchematicScene};
pub use symbol::SymbolScene;

/// Loads the application's default stroke font (upstream
/// `Application::getDefaultStrokeFont()`, `newstroke.bene`) from the
/// resources directory: `$LIBREPCB_SHARE/fontobene`, `../share/librepcb/fontobene`
/// relative to the executable, or the upstream checkout the crate was
/// built against. `None` if not found.
pub fn default_stroke_font() -> Option<librepcb_core::font::StrokeFont> {
    const FILE: &str = "fontobene/newstroke.bene";
    let mut dirs: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(dir) = std::env::var("LIBREPCB_SHARE") {
        dirs.push(dir.into());
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(bin) = exe.parent()
    {
        dirs.push(bin.join("../share/librepcb"));
    }
    dirs.push(std::path::Path::new(env!("LIBREPCB_UPSTREAM_DIR")).join("share/librepcb"));
    dirs.into_iter()
        .find_map(|dir| std::fs::read(dir.join(FILE)).ok())
        .map(librepcb_core::font::StrokeFont::new)
}

pub use librepcb_canvas;
