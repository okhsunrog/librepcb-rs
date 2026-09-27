//! Board mutations: boards (upstream `Project::addBoard()`,
//! `removeBoard()`, `Board::setName()`) and, in [`BoardMutation`], the
//! items (upstream `Board::addDeviceInstance()`,
//! `BI_NetSegment::addElements()`, `Board::addPlane()`, the item setters,
//! ...).
//!
//! TODO(wave3b/board): add the item mutations (`AddDevice`, `RemoveDevice`,
//! `SetDevicePlacement`, `AddNetSegment`, `AddNetSegmentElements`,
//! `RemoveNetSegmentElements`, `SetNetSegmentNet`, `AddPlane`, planes,
//! zones, polygons, stroke texts, holes, board settings, ...) with their
//! validation (pad/net/layer consistency, cohesion, one segment per pad),
//! reverse index updates (`p.refs.insert()`/`remove()`) and the dirty sets
//! of the derived data (air wires per net, plane layers).

use super::Mutation;
use crate::fileio::FileSystem;
use crate::project::Project;
use crate::project::board::{Board, BoardProperties};
use crate::project::change::Change;
use crate::project::error::{EntityKind, Error, Result};
use crate::project::id::BoardId;

/// A change of the items of a board, see
/// [`Mutation::Board`](super::Mutation::Board).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub enum BoardMutation {
    /// Placeholder until the items are ported (never applied successfully).
    #[doc(hidden)]
    Unimplemented(BoardId),
}

pub(super) fn apply(_p: &mut Project, m: BoardMutation) -> Result<Mutation> {
    // TODO(wave3b/board)
    match m {
        BoardMutation::Unimplemented(id) => Err(Error::NotFound {
            kind: EntityKind::Board,
            uuid: id.0,
        }),
    }
}

pub(super) fn add_board(p: &mut Project, board: Board, index: Option<usize>) -> Result<Mutation> {
    let id = board.id();
    if p.board(id).is_some() {
        return Err(Error::DuplicateUuid {
            kind: EntityKind::Board,
            uuid: id.0,
        });
    }
    check_board_name(p, board.name(), None)?;
    if p.boards
        .iter()
        .any(|b| b.directory_name() == board.directory_name())
    {
        return Err(Error::DuplicateDirectoryName {
            kind: EntityKind::Board,
            name: board.directory_name().to_owned(),
        });
    }
    // TODO(wave3b/board): validate the items against circuit and library
    // like `Board::addToProject()` (all items `addToBoard()`), then
    // `forceAirWiresRebuild()`.
    for r in board.references(id) {
        p.refs.insert(r);
    }
    let index = index.unwrap_or(p.boards.len()).min(p.boards.len());
    p.boards.insert(index, board);
    p.record(Change::BoardAdded(id));
    p.record(Change::ProjectMetadata);
    Ok(Mutation::RemoveBoard(id))
}

pub(super) fn remove_board(p: &mut Project, id: BoardId) -> Result<Mutation> {
    let index = p.board_index(id).ok_or(Error::NotFound {
        kind: EntityKind::Board,
        uuid: id.0,
    })?;
    // upstream: moves the board directory into a temporary file system
    p.directory
        .remove_dir_recursively(&format!("boards/{}", p.boards[index].directory_name()))?;
    let board = p.boards.remove(index);
    p.refs.remove_board(id);
    p.record(Change::BoardRemoved(id));
    p.record(Change::ProjectMetadata);
    Ok(Mutation::AddBoard {
        board,
        index: Some(index),
    })
}

pub(super) fn update_board(p: &mut Project, properties: BoardProperties) -> Result<Mutation> {
    let id = properties.id;
    let index = p.board_index(id).ok_or(Error::NotFound {
        kind: EntityKind::Board,
        uuid: id.0,
    })?;
    check_board_name(p, &properties.name, Some(id))?;
    let old = p.boards[index].set_properties(properties);
    p.record(Change::BoardChanged(id));
    p.record(Change::ProjectMetadata);
    Ok(Mutation::UpdateBoard(old))
}

fn check_board_name(p: &Project, name: &str, except: Option<BoardId>) -> Result<()> {
    if p.board_by_name(name)
        .is_some_and(|b| Some(b.id()) != except)
    {
        return Err(Error::DuplicateName {
            kind: EntityKind::Board,
            name: name.to_owned(),
        });
    }
    Ok(())
}
