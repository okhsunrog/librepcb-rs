//! The context the board editor states work in: the project editor, the
//! view and the FSM outputs (upstream `BoardEditorFsm::Context` and the
//! helpers of `BoardEditorState`).

use librepcb_core::project::board::Board;
use librepcb_core::project::{BoardId, Mutation, NetSignalId, Project};
use librepcb_core::types::{Layer, Point, PositiveLength};

use super::input::StatusMessage;
use super::output::{BoardFsmEvent, BoardFsmOutput};
use super::selection::BoardSelection;
use super::view::{BoardItemRef, BoardViewContext, FindFilter, FindFlags, find_items_at_pos};
use crate::commands::ApplyMutations;
use crate::editor::{Command, ProjectEditor};
use crate::error::{Error, Result};

/// What the application passes to every call of the FSM.
pub struct BoardFsmContext<'a> {
    /// The project editor (commands, undo stack).
    pub editor: &'a mut ProjectEditor,
    /// The view of the board (hit testing).
    pub view: &'a mut dyn BoardViewContext,
    /// The board being edited.
    pub board: BoardId,
}

impl std::fmt::Debug for BoardFsmContext<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BoardFsmContext")
            .field("board", &self.board)
            .finish_non_exhaustive()
    }
}

/// The context of a state while it processes an input.
pub(crate) struct Cx<'a, 'b> {
    pub ctx: &'a mut BoardFsmContext<'b>,
    pub out: &'a mut BoardFsmOutput,
    pub selection: &'a mut BoardSelection,
    pub ignore_locks: bool,
    /// Last known cursor position (upstream `QCursor::pos()` mapped to the
    /// scene).
    pub cursor_pos: Point,
    /// Set by a state to leave to the select tool after processing
    /// (upstream signal `requestLeavingState()`).
    pub leave_requested: bool,
}

impl Cx<'_, '_> {
    pub fn project(&self) -> &Project {
        self.ctx.editor.project()
    }

    pub fn board_id(&self) -> BoardId {
        self.ctx.board
    }

    pub fn board(&self) -> Result<&Board> {
        self.project()
            .board(self.ctx.board)
            .ok_or_else(|| Error::not_found("Board", self.ctx.board))
    }

    pub fn grid(&self) -> PositiveLength {
        super::view::grid_interval(self.project(), self.ctx.board)
    }

    pub fn inner_layer_count(&self) -> usize {
        self.board()
            .map(|b| b.settings().inner_layer_count as usize)
            .unwrap_or(0)
    }

    /// Copper layers of the board, sorted.
    pub fn copper_layers(&self) -> Vec<Layer> {
        self.board()
            .map(|b| b.copper_layers().into_iter().collect())
            .unwrap_or_default()
    }

    /// Upstream `BoardEditorState::findItemsAtPos()`.
    pub fn find_items(
        &self,
        pos: Point,
        flags: FindFlags,
        filter: &FindFilter<'_>,
    ) -> Vec<BoardItemRef> {
        find_items_at_pos(
            self.project(),
            self.ctx.board,
            &*self.ctx.view,
            pos,
            flags,
            filter,
        )
    }

    /// Upstream `BoardEditorState::findItemAtPos()`.
    pub fn find_item(
        &self,
        pos: Point,
        flags: FindFlags,
        filter: &FindFilter<'_>,
    ) -> Option<BoardItemRef> {
        self.find_items(pos, flags, filter).into_iter().next()
    }

    /// Updates the view after model changes.
    pub fn sync_view(&mut self) {
        let project = self.ctx.editor.project();
        self.ctx.view.sync(project);
    }

    pub fn is_group_active(&self) -> bool {
        self.ctx.editor.undo_stack().is_group_active()
    }

    pub fn group_len(&self) -> usize {
        self.ctx.editor.undo_stack().active_group_len().unwrap_or(0)
    }

    pub fn begin(&mut self, text: impl Into<String>) -> Result<()> {
        self.ctx.editor.begin_group(text)
    }

    pub fn commit(&mut self) -> Result<bool> {
        let result = self.ctx.editor.commit_group();
        self.sync_view();
        result
    }

    pub fn abort(&mut self) {
        if self.is_group_active()
            && let Err(e) = self.ctx.editor.abort_group()
        {
            log::error!("Failed to abort the command group: {e}");
        }
        self.sync_view();
    }

    /// Rolls back the active group to `len` operations.
    pub fn rollback_to(&mut self, len: usize) {
        self.ctx.editor.rollback_group_to(len);
    }

    /// Executes a command (in the active group, if any).
    pub fn exec<C: Command>(&mut self, command: C) -> Result<C::Output> {
        let result = self.ctx.editor.execute(command);
        self.sync_view();
        result
    }

    /// Applies mutations (in the active group, if any).
    pub fn apply(&mut self, text: String, mutations: Vec<Mutation>) -> Result<()> {
        if mutations.is_empty() {
            return Ok(());
        }
        self.exec(ApplyMutations {
            text: Some(text),
            mutations,
        })
    }

    /// Replaces the preview after `mark` in the active group by the given
    /// mutations.
    pub fn preview(&mut self, mark: usize, mutations: Vec<Mutation>) -> Result<()> {
        self.rollback_to(mark);
        let result = if mutations.is_empty() {
            Ok(())
        } else {
            self.ctx.editor.execute(ApplyMutations {
                text: None,
                mutations,
            })
        };
        self.rebuild_air_wires();
        self.sync_view();
        result
    }

    /// Rebuilds the scheduled air wires (upstream
    /// `triggerAirWiresRebuild()`).
    pub fn rebuild_air_wires(&mut self) {
        let board = self.ctx.board;
        if let Err(e) = self.ctx.editor.rebuild_air_wires(board) {
            log::warn!("Failed to rebuild air wires: {e}");
        }
    }

    /// Shows an error (upstream `QMessageBox::critical()`).
    pub fn error(&mut self, e: impl std::fmt::Display) {
        self.out.events.push(BoardFsmEvent::Error(e.to_string()));
    }

    /// Sets the status bar message.
    pub fn status(&mut self, text: impl Into<String>, timeout_ms: Option<u32>) {
        self.out
            .events
            .push(BoardFsmEvent::StatusMessage(StatusMessage {
                text: text.into(),
                timeout_ms,
            }));
    }

    /// Highlights nets (upstream `fsmCrossProbe()`).
    pub fn highlight_nets(&mut self, nets: impl IntoIterator<Item = NetSignalId>) {
        self.out.highlighted_nets = nets.into_iter().collect();
    }
}
