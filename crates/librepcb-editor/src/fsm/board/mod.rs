//! The board editor state machine: port of
//! libs/librepcb/editor/project/board/fsm/boardeditorfsm.{h,cpp},
//! boardeditorfsmadapter.h, boardeditorstate.{h,cpp} and the states
//! `boardeditorstate_*.{h,cpp}`, UI-toolkit independent.
//!
//! # Usage
//!
//! The application owns a [`BoardEditorFsm`] per board tab and calls
//! [`BoardEditorFsm::process()`] with a [`BoardFsmContext`] (the project
//! editor, its [`BoardViewContext`] implementation on the board scene, the
//! board) for every [`BoardFsmInput`]: pointer and key events in scene
//! coordinates, tool switches, actions (rotate, flip, delete, copy, ...)
//! and tool bar changes. Afterwards it shows [`BoardEditorFsm::output()`]
//! (cursor, info box, ruler, rubber band, highlighted nets, available
//! actions, tool bar data), mirrors [`BoardEditorFsm::selection()`] into
//! the scene and handles [`BoardEditorFsm::take_events()`] (errors,
//! messages, dialogs, clipboard, context menus).
//!
//! # Model changes and undo
//!
//! Every user action is one undo group of the [`ProjectEditor`]
//! (upstream `UndoStack::beginCmdGroup()`). Interactive tools (dragging,
//! drawing a trace, placing a via, device, text or hole, drawing planes,
//! polygons and zones) modify the project live inside the active group,
//! so the scene shows the preview through the change journal; they commit
//! the group when the action is finished (mouse release, click) and abort
//! it on [`BoardFsmInput::Abort`]. While a group is active, undo/redo and
//! other groups are blocked (like upstream). After each processed input,
//! the scheduled air wires are rebuilt.
//!
//! # Differences to upstream
//!
//! - The selection is held by the FSM ([`BoardSelection`]) instead of the
//!   graphics items; selecting a device implicitly selects its texts.
//! - Dialogs, message boxes and context menus are [`BoardFsmEvent`]s;
//!   answers come back as inputs ([`BoardFsmInput::ContextMenu`],
//!   [`BoardFsmInput::SetLineWidth`]).
//! - Tool bar values are polled ([`BoardToolData`]) instead of signals;
//!   changes are [`ToolSetting`]s.
//! - Previews are rolled back and re-applied on every change instead of
//!   modifying the model objects without undo commands.
//! - Not ported: the add-pad tools (standalone THT/SMT pads), DXF import,
//!   the "change device" context menu entries (need the workspace library
//!   database), `CmdSimplifyBoardNetSegments` after drawing and removing
//!   traces (not in core yet), aborting blocking tools in other editors
//!   (the application must abort them before switching tabs).

mod clipboard;
mod context;
mod input;
mod output;
mod selection;
mod transform;
mod view;

mod add_device;
mod add_hole;
mod add_stroke_text;
mod add_via;
mod draw_polygon;
mod draw_trace;
mod measure;
mod select;

use librepcb_core::project::ComponentInstanceId;
use librepcb_core::types::{Angle, Orientation, Point, UnsignedLength, Uuid};

pub use clipboard::{
    BoardClipboardData, ClipboardContent, ClipboardDevice, ClipboardNetSegment, ClipboardPlane,
    MIME_TYPE_PREFIX, PasteBoardItems,
};
pub use context::BoardFsmContext;
pub use input::{CursorShape, Key, Modifiers, PointerEvent, SceneCursor, StatusMessage};
pub use output::{
    BoardFeatures, BoardFsmEvent, BoardFsmOutput, BoardTool, BoardToolData, ContextAction,
    ContextMenuItem, ToolNet, ToolSetting, WireMode,
};
pub use selection::{BoardSelection, SelectionQuery};
pub use transform::DragItems;
pub use view::{BoardItemRef, BoardViewContext, FindFilter, FindFlags, find_items_at_pos};

use context::Cx;

#[allow(unused_imports)] // referenced by the documentation
use crate::editor::ProjectEditor;

/// An input of the board editor FSM (upstream `BoardEditorFsm::process*()`
/// methods).
#[derive(Debug, Clone, PartialEq)]
pub enum BoardFsmInput {
    /// Switch to a tool (upstream `processSelect()`, `processDrawTrace()`,
    /// ...). [`BoardTool::AddDevice`] is started with
    /// [`AddDevice`](Self::AddDevice).
    Tool(BoardTool),
    /// Start placing the device of a component (upstream
    /// `processAddDevice()`, from the "place devices" panel).
    AddDevice {
        /// The component.
        component: ComponentInstanceId,
        /// The library device.
        device: Uuid,
        /// The footprint (`None`: the first one).
        footprint: Option<Uuid>,
    },
    /// Abort the current command (Escape).
    Abort,
    /// Select all items.
    SelectAll,
    /// Cut the selection.
    Cut,
    /// Copy the selection; the cursor position is taken from the last
    /// pointer event.
    Copy,
    /// Paste the clipboard content.
    Paste(ClipboardContent),
    /// Move the selection by a vector (arrow keys).
    Move(Point),
    /// Rotate the selection (or the item being placed).
    Rotate(Angle),
    /// Flip the selection (or the item being placed).
    Flip(Orientation),
    /// Snap the selection to the grid.
    SnapToGrid,
    /// Lock or unlock the selection.
    SetLocked(bool),
    /// Change the line width of the selection to the next larger
    /// (`step > 0`) or smaller (`step < 0`) width used on the board, or ask
    /// for one (`0`).
    ChangeLineWidth(i32),
    /// The answer of [`BoardFsmEvent::RequestLineWidth`].
    SetLineWidth(UnsignedLength),
    /// Reset the texts of the selected devices.
    ResetAllTexts,
    /// Remove the selection.
    Remove,
    /// Open the properties of the selection.
    EditProperties,
    /// Add a plane around the board outline (plane tool, upstream
    /// `autoAdd()`).
    AutoAddPlane,
    /// A key was pressed.
    KeyPressed(Key, Modifiers),
    /// A key was released.
    KeyReleased(Key, Modifiers),
    /// The pointer moved.
    PointerMoved(PointerEvent),
    /// The left button was pressed.
    LeftPressed(PointerEvent),
    /// The left button was released.
    LeftReleased(PointerEvent),
    /// The left button was double-clicked.
    LeftDoubleClicked(PointerEvent),
    /// The right button was released (without panning).
    RightReleased(PointerEvent),
    /// A context menu entry was chosen.
    ContextMenu(ContextAction),
    /// A tool bar value was changed.
    ToolSetting(ToolSetting),
}

/// Interface of the states (upstream `BoardEditorState`).
trait State {
    /// Enters the state; `false` if it cannot be entered.
    fn entry(&mut self, _cx: &mut Cx<'_, '_>) -> bool {
        true
    }
    /// Leaves the state; `false` if it cannot be left.
    fn exit(&mut self, _cx: &mut Cx<'_, '_>) -> bool {
        true
    }
    /// Processes an input; `true` if handled.
    fn process(&mut self, cx: &mut Cx<'_, '_>, input: &BoardFsmInput) -> bool;
    /// Tool bar data.
    fn tool_data(&self, _cx: &Cx<'_, '_>) -> BoardToolData {
        BoardToolData::default()
    }
}

/// The board editor state machine (upstream `BoardEditorFsm`).
#[derive(Debug)]
pub struct BoardEditorFsm {
    current: Option<BoardTool>,
    previous: Option<BoardTool>,
    select: select::SelectState,
    draw_trace: draw_trace::DrawTraceState,
    add_via: add_via::AddViaState,
    draw_polygon: draw_polygon::DrawPolygonState,
    draw_plane: draw_polygon::DrawPlaneState,
    draw_zone: draw_polygon::DrawZoneState,
    add_hole: add_hole::AddHoleState,
    add_stroke_text: add_stroke_text::AddStrokeTextState,
    add_device: add_device::AddDeviceState,
    measure: measure::MeasureState,
    output: BoardFsmOutput,
    selection: BoardSelection,
    ignore_locks: bool,
    cursor_pos: Point,
}

impl BoardEditorFsm {
    /// Creates the FSM and enters the select tool.
    pub fn new(ctx: &mut BoardFsmContext<'_>) -> Self {
        let default_width = ctx
            .editor
            .project()
            .board(ctx.board)
            .map(|b| b.design_rules().default_trace_width());
        let mut fsm = Self {
            current: None,
            previous: None,
            select: select::SelectState::default(),
            draw_trace: draw_trace::DrawTraceState::new(default_width),
            add_via: add_via::AddViaState::default(),
            draw_polygon: draw_polygon::DrawPolygonState::default(),
            draw_plane: draw_polygon::DrawPlaneState::default(),
            draw_zone: draw_polygon::DrawZoneState::default(),
            add_hole: add_hole::AddHoleState::default(),
            add_stroke_text: add_stroke_text::AddStrokeTextState::default(),
            add_device: add_device::AddDeviceState::default(),
            measure: measure::MeasureState::default(),
            output: BoardFsmOutput::default(),
            selection: BoardSelection::default(),
            ignore_locks: false,
            cursor_pos: Point::ORIGIN,
        };
        fsm.with_cx(ctx, |fsm, cx| {
            fsm.enter(cx, BoardTool::Select);
        });
        fsm
    }

    /// The current tool.
    pub fn tool(&self) -> BoardTool {
        self.current.unwrap_or_default()
    }

    /// The view state and tool bar data.
    pub fn output(&self) -> &BoardFsmOutput {
        &self.output
    }

    /// Takes the events produced since the last call.
    pub fn take_events(&mut self) -> Vec<BoardFsmEvent> {
        std::mem::take(&mut self.output.events)
    }

    /// The selection.
    pub fn selection(&self) -> &BoardSelection {
        &self.selection
    }

    /// Whether locked items can be modified (upstream "ignore placement
    /// locks" of the tab).
    pub fn set_ignore_locks(&mut self, ignore: bool) {
        self.ignore_locks = ignore;
    }

    /// Replaces the selection (e.g. from the "find" feature or cross
    /// probing), upstream `processChangedSelection()`.
    pub fn set_selection(
        &mut self,
        ctx: &mut BoardFsmContext<'_>,
        items: impl IntoIterator<Item = BoardItemRef>,
    ) {
        self.selection.replace(items);
        self.with_cx(ctx, |fsm, cx| {
            if fsm.current == Some(BoardTool::Select) {
                fsm.select.update_features(cx, false);
            }
        });
    }

    /// Processes an input; returns whether it was handled.
    pub fn process(&mut self, ctx: &mut BoardFsmContext<'_>, input: BoardFsmInput) -> bool {
        if let BoardFsmInput::PointerMoved(e)
        | BoardFsmInput::LeftPressed(e)
        | BoardFsmInput::LeftReleased(e)
        | BoardFsmInput::LeftDoubleClicked(e)
        | BoardFsmInput::RightReleased(e) = &input
        {
            self.cursor_pos = e.pos;
        }
        self.with_cx(ctx, |fsm, cx| fsm.dispatch(cx, &input))
    }

    fn with_cx<R>(
        &mut self,
        ctx: &mut BoardFsmContext<'_>,
        f: impl FnOnce(&mut Self, &mut Cx<'_, '_>) -> R,
    ) -> R {
        let mut out = std::mem::take(&mut self.output);
        let mut selection = std::mem::take(&mut self.selection);
        let (result, leave) = {
            let mut cx = Cx {
                ctx,
                out: &mut out,
                selection: &mut selection,
                ignore_locks: self.ignore_locks,
                cursor_pos: self.cursor_pos,
                leave_requested: false,
            };
            let result = f(self, &mut cx);
            let mut leave = cx.leave_requested;
            // Upstream: `requestLeavingState()` is queued.
            while leave {
                cx.leave_requested = false;
                self.set_state(&mut cx, BoardTool::Select);
                leave = cx.leave_requested;
            }
            // Keep the selection valid after model changes.
            if let Ok(board) = cx.board() {
                let board = board.clone();
                cx.selection.update(&board);
            }
            // Air wires and tool bar data.
            if !cx.is_group_active() {
                cx.rebuild_air_wires();
                cx.sync_view();
            }
            let mut data = self
                .state_ref()
                .map(|s| s.tool_data(&cx))
                .unwrap_or_default();
            data.tool = self.tool();
            cx.out.tool = data;
            (result, leave)
        };
        let _ = leave;
        self.output = out;
        self.selection = selection;
        result
    }

    fn state_mut(&mut self, tool: BoardTool) -> &mut dyn State {
        match tool {
            BoardTool::Select => &mut self.select,
            BoardTool::DrawTrace => &mut self.draw_trace,
            BoardTool::AddVia => &mut self.add_via,
            BoardTool::DrawPolygon => &mut self.draw_polygon,
            BoardTool::DrawPlane => &mut self.draw_plane,
            BoardTool::DrawZone => &mut self.draw_zone,
            BoardTool::AddHole => &mut self.add_hole,
            BoardTool::AddStrokeText => &mut self.add_stroke_text,
            BoardTool::AddDevice => &mut self.add_device,
            BoardTool::Measure => &mut self.measure,
        }
    }

    fn state_ref(&self) -> Option<&dyn State> {
        Some(match self.current? {
            BoardTool::Select => &self.select,
            BoardTool::DrawTrace => &self.draw_trace,
            BoardTool::AddVia => &self.add_via,
            BoardTool::DrawPolygon => &self.draw_polygon,
            BoardTool::DrawPlane => &self.draw_plane,
            BoardTool::DrawZone => &self.draw_zone,
            BoardTool::AddHole => &self.add_hole,
            BoardTool::AddStrokeText => &self.add_stroke_text,
            BoardTool::AddDevice => &self.add_device,
            BoardTool::Measure => &self.measure,
        })
    }

    fn dispatch(&mut self, cx: &mut Cx<'_, '_>, input: &BoardFsmInput) -> bool {
        match input {
            BoardFsmInput::Tool(tool) => {
                if *tool == BoardTool::AddDevice {
                    return false;
                }
                self.set_state(cx, *tool)
            }
            BoardFsmInput::AddDevice { .. } => {
                // upstream `processAddDevice()`
                let old = self.tool();
                if !self.set_state(cx, BoardTool::AddDevice) {
                    return false;
                }
                if self.add_device.process(cx, input) {
                    return true;
                }
                self.set_state(cx, old);
                false
            }
            BoardFsmInput::Abort => {
                let handled = self
                    .current
                    .is_some_and(|t| self.state_mut(t).process(cx, input));
                handled || self.set_state(cx, BoardTool::Select)
            }
            BoardFsmInput::RightReleased(_) => {
                let handled = self
                    .current
                    .is_some_and(|t| self.state_mut(t).process(cx, input));
                if handled {
                    true
                } else if self.current != Some(BoardTool::Select) {
                    // If a right click is not handled, abort the command.
                    self.dispatch(cx, &BoardFsmInput::Abort)
                } else {
                    // In the select state, switch back to the last state.
                    let next = match self.previous {
                        Some(p) if Some(p) != self.current => p,
                        _ => BoardTool::Select,
                    };
                    self.set_state(cx, next)
                }
            }
            _ => self
                .current
                .is_some_and(|t| self.state_mut(t).process(cx, input)),
        }
    }

    fn set_state(&mut self, cx: &mut Cx<'_, '_>, tool: BoardTool) -> bool {
        if self.current == Some(tool) {
            return true;
        }
        if let Some(current) = self.current {
            if !self.state_mut(current).exit(cx) {
                return false;
            }
            // Only memorize states other than select and add device.
            if !matches!(current, BoardTool::Select | BoardTool::AddDevice) {
                self.previous = Some(current);
            }
            self.current = None;
        }
        self.enter(cx, tool)
    }

    fn enter(&mut self, cx: &mut Cx<'_, '_>, tool: BoardTool) -> bool {
        if !self.state_mut(tool).entry(cx) {
            return false;
        }
        self.current = Some(tool);
        true
    }
}
