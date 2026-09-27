//! Board part of the project loader (upstream
//! `ProjectLoader::loadBoards()`, `loadBoard()`,
//! `loadBoardDeviceInstance()`, `loadBoardNetSegment()`,
//! `loadBoardPlane()`, `loadBoardUserSettings()`).
//!
//! TODO(wave3b/board): load the settings (grid, layers, thickness, colors,
//! design rules, DRC settings and approvals, fabrication output settings,
//! preferred footprint tags), the items (devices with stroke texts, net
//! segments with the anchor resolution and its errors, planes, zones,
//! polygons, stroke texts, holes), the user settings (layer and plane
//! visibility) and the `auto_assign_device_models` option; drop the
//! verbatim `raw` copies.

use super::{parse_file, split_index_path};
use crate::project::Project;
use crate::project::board::Board;
use crate::project::error::Result;

pub(super) fn load_boards(p: &mut Project) -> Result<()> {
    let index = parse_file(p, "boards/boards.lp")?;
    for node in index.children_named("board") {
        let relative: String = node.child_value("@0")?;
        load_board(p, &relative)?;
    }
    log::debug!("Successfully loaded boards.");
    Ok(())
}

fn load_board(p: &mut Project, relative_file_path: &str) -> Result<()> {
    let (dir, directory_name) = split_index_path(relative_file_path);
    let root = parse_file(p, relative_file_path)?;
    let mut board = Board::new(
        root.child_value("@0")?,
        root.child_value("name/@0")?,
        directory_name,
    );
    board.raw = Some(root);
    // TODO(wave3b/board): settings and items.
    board.raw_user_settings = match parse_file(p, &format!("{dir}/settings.user.lp")) {
        Ok(root) => Some(root),
        Err(e) => {
            log::error!("Could not load board user settings, defaults will be used instead: {e}");
            None
        }
    };
    p.add_board(board, None)?;
    Ok(())
}
