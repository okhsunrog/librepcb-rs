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
//! - [`render_schematic_png()`] and [`render_board_png()`] render a page or
//!   a board side into a PNG, fitted to the content ([`RenderOptions`]).
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
mod render;
mod schematic;
mod shapes;
mod text;

pub use board::{BoardObject, BoardScene, BoardSceneLayer, BoardSide};
pub use colors::ColorScheme;
pub use error::{Error, Result};
pub use render::{
    MAX_IMAGE_SIZE, RenderOptions, RenderSize, RgbaImage, render_board_png, render_board_scene,
    render_scene, render_schematic_png, render_schematic_scene, visible_bounds,
};
pub use schematic::{SchematicObject, SchematicScene};

pub use librepcb_canvas;
