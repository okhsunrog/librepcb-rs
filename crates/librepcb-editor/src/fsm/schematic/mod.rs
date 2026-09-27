//! The schematic editor state machine: port of
//! libs/librepcb/editor/project/schematic/fsm/schematiceditorfsm.{h,cpp},
//! schematiceditorfsmadapter.h and schematiceditorstate.{h,cpp}, with the
//! states in the submodules (`select`, `draw_wire`, `add_label`,
//! `add_component`, `add_text`, `draw_polygon`, `measure`).
//!
//! Usage: the application creates one [`SchematicEditorFsm`] per schematic
//! tab and calls its methods with a [`SchematicContext`] (the project
//! editor, the view used for hit testing and the clipboard). Afterwards it
//! syncs its scene with the project's change journal, reflects
//! [`selection()`](SchematicEditorFsm::selection) in the scene and shows
//! [`view_state()`](SchematicEditorFsm::view_state) and
//! [`tool_data()`](SchematicEditorFsm::tool_data); dialogs are requested
//! through [`take_requests()`](SchematicEditorFsm::take_requests).
//!
//! Differences to upstream:
//!
//! - The FSM owns the selection (a set of [`SchematicItem`]s) instead of
//!   the graphics items; the view only answers hit tests
//!   ([`SchematicView`]). Junctions are hit tested on the model (upstream:
//!   invisible graphics items), so the view does not need to report them.
//! - Interactive operations keep an undo group open and replace their live
//!   preview inside it (`ProjectEditor::rollback_group_to()`) instead of
//!   modifying the model without undo commands (`immediate` edits).
//! - A drag (and paste) is one undo group including the simplification of
//!   the modified net segments (upstream: two undo steps).
//! - Error message boxes become [`SchematicRequest::ShowError`], the
//!   context menu becomes [`SchematicRequest::ContextMenu`].
//! - Not ported: buses (drawing, bus labels, splitting bus lines), images
//!   (adding, resizing), moving polygon vertices, cross-probing, the "find"
//!   feature, symbol texts as separately selectable items.

mod add_component;
mod add_label;
mod add_text;
pub mod clipboard;
mod drag;
mod draw_polygon;
mod draw_wire;
mod hit_test;
mod measure;
mod select;
mod selection;
mod simplify;

use std::collections::BTreeSet;

use librepcb_core::project::schematic::Schematic;
use librepcb_core::project::{
    BusSegmentId, ComponentInstanceId, NetSegmentId, Project, SchematicId, SymbolId,
};
use librepcb_core::types::{
    Angle, Layer, Length, LengthUnit, Orientation, Point, PositiveLength, UnsignedLength, Uuid,
};

use super::{Clipboard, CursorShape, KeyEvent, PointerEvent, ViewState};
use crate::ProjectEditor;
use crate::commands::WireMode;

pub use clipboard::{SCHEMATIC_CLIPBOARD_MIME_PREFIX, SchematicClipboardData};
pub use simplify::SimplifySchematicSegments;

/// An item of a schematic page as the editor sees it (upstream: the
/// graphics items of `SchematicGraphicsScene`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SchematicItem {
    /// A symbol.
    Symbol(SymbolId),
    /// A pin of a symbol (library pin UUID).
    SymbolPin(SymbolId, Uuid),
    /// A junction of a net segment (upstream `SI_NetPoint`).
    NetPoint(NetSegmentId, Uuid),
    /// A net line.
    NetLine(NetSegmentId, Uuid),
    /// A net label.
    NetLabel(NetSegmentId, Uuid),
    /// A junction of a bus segment.
    BusJunction(BusSegmentId, Uuid),
    /// A line of a bus segment.
    BusLine(BusSegmentId, Uuid),
    /// A label of a bus segment.
    BusLabel(BusSegmentId, Uuid),
    /// A polygon.
    Polygon(Uuid),
    /// A text of the page.
    Text(Uuid),
    /// An image.
    Image(Uuid),
}

/// What the FSM needs from the view: hit testing on the displayed scene
/// (upstream `SchematicGraphicsScene` and `fsmCalcPosWithTolerance()`).
///
/// The application implements it on its `librepcb_scene::SchematicScene`
/// and canvas view (mapping `SchematicObject::NetJunction(seg,
/// Junction(j))` to [`SchematicItem::NetPoint`] and `NetJunction(_, Pin
/// {..})` to [`SchematicItem::SymbolPin`]); the order of the returned items
/// does not matter.
pub trait SchematicView {
    /// All items whose grab area is at most `tolerance` away from `pos`
    /// (`tolerance == 0`: exactly under `pos`).
    fn items_at(&self, pos: Point, tolerance: Length) -> Vec<SchematicItem>;

    /// All items touching the rectangle spanned by `p1` and `p2`.
    fn items_in_rect(&self, p1: Point, p2: Point) -> Vec<SchematicItem>;

    /// The hit tolerance: upstream uses 5 screen pixels, so this is the
    /// world length of 5 pixels at the current zoom level (see
    /// [`tolerance_for_pixel_size()`](super::tolerance_for_pixel_size)).
    fn tolerance(&self) -> Length;

    /// The current cursor position in world coordinates, if known (used
    /// when a tool starts without pointer event, e.g. paste or add
    /// component; upstream `QCursor::pos()`).
    fn cursor_pos(&self) -> Option<Point> {
        None
    }
}

/// The context of an FSM call.
pub struct SchematicContext<'a> {
    /// The project editor (commands and undo groups).
    pub editor: &'a mut ProjectEditor,
    /// Hit testing on the view.
    pub view: &'a dyn SchematicView,
    /// The clipboard.
    pub clipboard: &'a mut dyn Clipboard,
}

impl<'a> SchematicContext<'a> {
    /// Creates a context.
    pub fn new(
        editor: &'a mut ProjectEditor,
        view: &'a dyn SchematicView,
        clipboard: &'a mut dyn Clipboard,
    ) -> Self {
        Self {
            editor,
            view,
            clipboard,
        }
    }
}

/// The active tool (upstream `SchematicEditorFsm::State`, Slint
/// `EditorTool`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum SchematicTool {
    /// Select, move, rotate, ... (upstream `SchematicEditorState_Select`).
    #[default]
    Select,
    /// Draw wires.
    Wire,
    /// Add net labels.
    Label,
    /// Add components.
    Component,
    /// Draw polygons.
    Polygon,
    /// Add texts.
    Text,
    /// Measure distances.
    Measure,
}

/// The tool bar data of the active tool (the `tool-*` fields of upstream's
/// `SchematicTabData`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchematicToolData {
    /// Wire mode (draw wire).
    pub wire_mode: WireMode,
    /// Layer (draw polygon, add text).
    pub layer: Layer,
    /// Layers to choose from (draw polygon, add text).
    pub available_layers: Vec<Layer>,
    /// Line width (draw polygon).
    pub line_width: UnsignedLength,
    /// Text height (add text).
    pub size: PositiveLength,
    /// Whether polygons are filled (draw polygon).
    pub filled: bool,
    /// Text (add text) or value (add component).
    pub value: String,
    /// Suggestions for [`value`](Self::value).
    pub value_suggestions: Vec<String>,
}

impl Default for SchematicToolData {
    fn default() -> Self {
        Self {
            wire_mode: WireMode::HV,
            layer: Layer::SCHEMATIC_GUIDE,
            available_layers: Vec::new(),
            line_width: UnsignedLength::new(Length::new(300_000)).unwrap_or_default(),
            size: default_text_height(),
            filled: false,
            value: String::new(),
            value_suggestions: Vec::new(),
        }
    }
}

fn default_text_height() -> PositiveLength {
    // 2.5 mm is positive.
    PositiveLength::new(Length::new(2_500_000)).unwrap_or_else(|_| unreachable!())
}

/// A request of the FSM to the application (dialogs and menus upstream
/// opens itself).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchematicRequest {
    /// Show an error message (upstream `QMessageBox::critical()`).
    ShowError(String),
    /// Open the "add component" dialog; the application calls
    /// [`SchematicEditorFsm::add_component()`] with the choice.
    AddComponentDialog {
        /// Search term to preselect (e.g. "schematic frame").
        search_term: String,
    },
    /// Open the properties dialog of a symbol (component).
    SymbolProperties(SymbolId),
    /// Open the rename dialog of the net segment of a net label.
    NetLabelProperties(NetSegmentId, Uuid),
    /// Open the properties dialog of a polygon.
    PolygonProperties(Uuid),
    /// Open the properties dialog of a text.
    TextProperties(Uuid),
    /// Show the context menu for the (selected) item at `pos`; its actions
    /// map to the FSM methods (properties, cut, copy, remove, rotate,
    /// mirror, snap to grid, reset texts, place remaining gates).
    ContextMenu {
        /// The item under the cursor.
        item: SchematicItem,
        /// The position (world coordinates).
        pos: Point,
    },
    /// Zoom to show all items (e.g. after adding a drawing frame).
    ZoomAll,
}

/// Settings of the schematic editor FSM.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchematicEditorSettings {
    /// Ignore placement locks of texts (upstream "ignore placement locks").
    pub ignore_locks: bool,
    /// Application version used in the clipboard MIME type (upstream
    /// `Application::getVersion()`; clipboard data is only exchanged
    /// between identical versions).
    pub app_version: String,
}

impl Default for SchematicEditorSettings {
    fn default() -> Self {
        Self {
            ignore_locks: false,
            app_version: "2.1.2-unstable".to_owned(),
        }
    }
}

/// A component to add (the choice of upstream's "add component" dialog or
/// tool bar buttons).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentChoice {
    /// The library component.
    pub component: Uuid,
    /// The symbol variant (default: by norm order).
    pub symbol_variant: Option<Uuid>,
    /// The library device for an assembly option (default: none).
    pub device: Option<Uuid>,
}

impl ComponentChoice {
    /// A component with default symbol variant and without device.
    pub fn new(component: Uuid) -> Self {
        Self {
            component,
            symbol_variant: None,
            device: None,
        }
    }
}

/// The outputs of the FSM, shared by all states.
#[derive(Debug, Default)]
pub(crate) struct Output {
    pub view: ViewState,
    pub selection: BTreeSet<SchematicItem>,
    pub hovered: Option<SchematicItem>,
    pub tool: SchematicTool,
    pub tool_data: SchematicToolData,
    pub requests: Vec<SchematicRequest>,
    /// A state asked to go back to the select state (upstream
    /// `requestLeavingState()`, processed after the call).
    pub leave_requested: bool,
    /// The last pointer position.
    pub last_pos: Option<Point>,
}

/// What a state gets in its handlers.
pub(crate) struct Cx<'c, 'a> {
    pub ctx: &'c mut SchematicContext<'a>,
    pub out: &'c mut Output,
    pub schematic: SchematicId,
    pub settings: &'c SchematicEditorSettings,
}

impl Cx<'_, '_> {
    pub fn project(&self) -> &Project {
        self.ctx.editor.project()
    }

    pub fn sch(&self) -> Option<&Schematic> {
        self.ctx.editor.project().schematic(self.schematic)
    }

    /// Grid interval of the page (upstream `getGridInterval()`).
    pub fn grid(&self) -> PositiveLength {
        self.sch()
            .map(|s| s.properties().grid_interval)
            .unwrap_or_else(|| {
                PositiveLength::new(Length::new(2_540_000)).unwrap_or_else(|_| unreachable!())
            })
    }

    /// Length unit of the page (upstream `getLengthUnit()`).
    pub fn unit(&self) -> LengthUnit {
        self.sch()
            .map(|s| s.properties().grid_unit)
            .unwrap_or_default()
    }

    /// The cursor position (from the view, else the last pointer event).
    pub fn cursor_pos(&self) -> Point {
        self.ctx
            .view
            .cursor_pos()
            .or(self.out.last_pos)
            .unwrap_or_default()
    }

    /// Reports an error (upstream: message box).
    pub fn error(&mut self, e: impl std::fmt::Display) {
        let msg = e.to_string();
        log::warn!("Schematic editor: {msg}");
        self.out.requests.push(SchematicRequest::ShowError(msg));
    }

    pub fn set_cursor(&mut self, shape: Option<CursorShape>) {
        self.out.view.cursor = shape;
    }
}

/// The states of the FSM (upstream `SchematicEditorFsm::State`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StateKind {
    Idle,
    Select,
    DrawWire,
    AddLabel,
    AddComponent,
    DrawPolygon,
    AddText,
    Measure,
}

/// The event handlers of a state (upstream `SchematicEditorState`); each
/// returns whether the event was handled.
#[allow(unused_variables)]
pub(crate) trait State {
    fn entry(&mut self, cx: &mut Cx<'_, '_>) -> bool {
        true
    }
    fn exit(&mut self, cx: &mut Cx<'_, '_>) -> bool {
        true
    }
    fn abort(&mut self, cx: &mut Cx<'_, '_>) -> bool {
        false
    }
    fn select_all(&mut self, cx: &mut Cx<'_, '_>) -> bool {
        false
    }
    fn cut(&mut self, cx: &mut Cx<'_, '_>) -> bool {
        false
    }
    fn copy(&mut self, cx: &mut Cx<'_, '_>) -> bool {
        false
    }
    fn paste(&mut self, cx: &mut Cx<'_, '_>) -> bool {
        false
    }
    fn move_by(&mut self, cx: &mut Cx<'_, '_>, delta: Point) -> bool {
        false
    }
    fn rotate(&mut self, cx: &mut Cx<'_, '_>, angle: Angle) -> bool {
        false
    }
    fn mirror(&mut self, cx: &mut Cx<'_, '_>, orientation: Orientation) -> bool {
        false
    }
    fn snap_to_grid(&mut self, cx: &mut Cx<'_, '_>) -> bool {
        false
    }
    fn reset_all_texts(&mut self, cx: &mut Cx<'_, '_>) -> bool {
        false
    }
    fn remove(&mut self, cx: &mut Cx<'_, '_>) -> bool {
        false
    }
    fn edit_properties(&mut self, cx: &mut Cx<'_, '_>) -> bool {
        false
    }
    fn key_pressed(&mut self, cx: &mut Cx<'_, '_>, e: KeyEvent) -> bool {
        false
    }
    fn key_released(&mut self, cx: &mut Cx<'_, '_>, e: KeyEvent) -> bool {
        false
    }
    fn pointer_moved(&mut self, cx: &mut Cx<'_, '_>, e: PointerEvent) -> bool {
        false
    }
    fn left_pressed(&mut self, cx: &mut Cx<'_, '_>, e: PointerEvent) -> bool {
        false
    }
    fn left_released(&mut self, cx: &mut Cx<'_, '_>, e: PointerEvent) -> bool {
        false
    }
    fn left_double_clicked(&mut self, cx: &mut Cx<'_, '_>, e: PointerEvent) -> bool {
        false
    }
    fn right_released(&mut self, cx: &mut Cx<'_, '_>, e: PointerEvent) -> bool {
        false
    }
    /// Called after every event while the state is active (feature and
    /// info box updates, upstream: timers and signals).
    fn update(&mut self, cx: &mut Cx<'_, '_>) {}
}

#[derive(Debug, Default)]
struct States {
    select: select::SelectState,
    draw_wire: draw_wire::DrawWireState,
    add_label: add_label::AddLabelState,
    add_component: add_component::AddComponentState,
    draw_polygon: draw_polygon::DrawPolygonState,
    add_text: add_text::AddTextState,
    measure: measure::MeasureState,
}

impl States {
    fn get(&mut self, kind: StateKind) -> Option<&mut dyn State> {
        Some(match kind {
            StateKind::Idle => return None,
            StateKind::Select => &mut self.select,
            StateKind::DrawWire => &mut self.draw_wire,
            StateKind::AddLabel => &mut self.add_label,
            StateKind::AddComponent => &mut self.add_component,
            StateKind::DrawPolygon => &mut self.draw_polygon,
            StateKind::AddText => &mut self.add_text,
            StateKind::Measure => &mut self.measure,
        })
    }
}

/// The schematic editor state machine of one schematic page (upstream
/// `SchematicEditorFsm`).
#[derive(Debug)]
pub struct SchematicEditorFsm {
    schematic: SchematicId,
    settings: SchematicEditorSettings,
    current: StateKind,
    previous: StateKind,
    entered: bool,
    states: States,
    out: Output,
}

macro_rules! dispatch {
    ($self:ident, $ctx:ident, |$state:ident, $cx:ident| $body:expr) => {{
        let kind = $self.ensure_entered($ctx);
        let handled = {
            let mut $cx = Cx {
                ctx: $ctx,
                out: &mut $self.out,
                schematic: $self.schematic,
                settings: &$self.settings,
            };
            match $self.states.get(kind) {
                Some($state) => $body,
                None => false,
            }
        };
        $self.after_event($ctx);
        handled
    }};
}

impl SchematicEditorFsm {
    /// Creates the FSM for a schematic page; it starts in the select
    /// state.
    pub fn new(schematic: SchematicId, settings: SchematicEditorSettings) -> Self {
        Self {
            schematic,
            settings,
            current: StateKind::Select,
            previous: StateKind::Idle,
            entered: false,
            states: States::default(),
            out: Output::default(),
        }
    }

    /// The schematic page.
    pub fn schematic(&self) -> SchematicId {
        self.schematic
    }

    /// The settings.
    pub fn settings(&self) -> &SchematicEditorSettings {
        &self.settings
    }

    /// Changes the settings.
    pub fn set_settings(&mut self, settings: SchematicEditorSettings) {
        self.settings = settings;
    }

    // --- Outputs ---

    /// The active tool.
    pub fn tool(&self) -> SchematicTool {
        self.out.tool
    }

    /// The tool bar data of the active tool.
    pub fn tool_data(&self) -> &SchematicToolData {
        &self.out.tool_data
    }

    /// What the view shows besides the scene.
    pub fn view_state(&self) -> &ViewState {
        &self.out.view
    }

    /// Takes the status bar message set since the last call.
    pub fn take_status_message(&mut self) -> Option<super::StatusMessage> {
        self.out.view.status_message.take()
    }

    /// The selected items.
    pub fn selection(&self) -> &BTreeSet<SchematicItem> {
        &self.out.selection
    }

    /// Replaces the selection (e.g. cross-probing, "find").
    pub fn set_selection(&mut self, items: impl IntoIterator<Item = SchematicItem>) {
        self.out.selection = items.into_iter().collect();
    }

    /// Removes items which no longer exist from the selection; call after
    /// changing the project outside the FSM (undo, redo, MCP). While a tool
    /// keeps an undo group open (dragging, drawing, placing), undo and redo
    /// are not possible: call [`abort()`](Self::abort) first.
    pub fn project_changed(&mut self, project: &Project) {
        if let Some(s) = project.schematic(self.schematic) {
            self.out
                .selection
                .retain(|item| selection::item_exists(item, s));
        }
    }

    /// The item under the cursor in the select state (for highlighting).
    pub fn hovered(&self) -> Option<SchematicItem> {
        self.out.hovered
    }

    /// Takes the pending requests (dialogs, errors, menus).
    pub fn take_requests(&mut self) -> Vec<SchematicRequest> {
        std::mem::take(&mut self.out.requests)
    }

    // --- Tool switching ---

    /// Switches to the select tool (upstream `processSelect()`).
    pub fn select_tool(&mut self, ctx: &mut SchematicContext<'_>) -> bool {
        self.ensure_entered(ctx);
        let ok = self.set_next_state(ctx, StateKind::Select);
        self.after_event(ctx);
        ok
    }

    /// Switches to the draw wire tool.
    pub fn draw_wire(&mut self, ctx: &mut SchematicContext<'_>) -> bool {
        self.switch_to(ctx, StateKind::DrawWire)
    }

    /// Switches to the add net label tool.
    pub fn add_net_label(&mut self, ctx: &mut SchematicContext<'_>) -> bool {
        self.switch_to(ctx, StateKind::AddLabel)
    }

    /// Switches to the draw polygon tool.
    pub fn draw_polygon(&mut self, ctx: &mut SchematicContext<'_>) -> bool {
        self.switch_to(ctx, StateKind::DrawPolygon)
    }

    /// Switches to the add text tool (a text follows the cursor).
    pub fn add_text(&mut self, ctx: &mut SchematicContext<'_>) -> bool {
        self.switch_to(ctx, StateKind::AddText)
    }

    /// Switches to the measure tool.
    pub fn measure(&mut self, ctx: &mut SchematicContext<'_>) -> bool {
        self.switch_to(ctx, StateKind::Measure)
    }

    /// Requests the "add component" dialog (upstream
    /// `processAddComponent(searchTerm)`): emits
    /// [`SchematicRequest::AddComponentDialog`]; the application then calls
    /// [`add_component()`](Self::add_component).
    pub fn open_add_component_dialog(&mut self, search_term: impl Into<String>) {
        self.out
            .requests
            .push(SchematicRequest::AddComponentDialog {
                search_term: search_term.into(),
            });
    }

    /// Starts adding a component (upstream `processAddComponent(cmp,
    /// symbVar)`): the first gate follows the cursor; each click places a
    /// gate, then the next component of the same kind is added.
    pub fn add_component(
        &mut self,
        ctx: &mut SchematicContext<'_>,
        choice: ComponentChoice,
    ) -> bool {
        let old = self.ensure_entered(ctx);
        if !self.set_next_state(ctx, StateKind::AddComponent) {
            self.after_event(ctx);
            return false;
        }
        let ok = {
            let mut cx = Cx {
                ctx,
                out: &mut self.out,
                schematic: self.schematic,
                settings: &self.settings,
            };
            self.states.add_component.start(&mut cx, choice)
        };
        if !ok {
            self.set_next_state(ctx, old);
        }
        self.after_event(ctx);
        ok
    }

    /// Places the remaining gates of a component (or only `gate`), upstream
    /// `processAddRemainingGates()`.
    pub fn add_remaining_gates(
        &mut self,
        ctx: &mut SchematicContext<'_>,
        component: ComponentInstanceId,
        gate: Option<Uuid>,
    ) -> bool {
        let old = self.ensure_entered(ctx);
        let ok = self.add_remaining_gates_impl(ctx, component, gate, old);
        self.after_event(ctx);
        ok
    }

    fn add_remaining_gates_impl(
        &mut self,
        ctx: &mut SchematicContext<'_>,
        component: ComponentInstanceId,
        gate: Option<Uuid>,
        old: StateKind,
    ) -> bool {
        if !self.set_next_state(ctx, StateKind::AddComponent) {
            return false;
        }
        let ok = {
            let mut cx = Cx {
                ctx,
                out: &mut self.out,
                schematic: self.schematic,
                settings: &self.settings,
            };
            self.states
                .add_component
                .start_remaining_gates(&mut cx, component, gate)
        };
        if !ok {
            self.set_next_state(ctx, old);
        }
        ok
    }

    // --- Actions ---

    /// Aborts the current command, or goes back to the select tool
    /// (upstream `processAbortCommand()`, Escape).
    pub fn abort(&mut self, ctx: &mut SchematicContext<'_>) -> bool {
        let kind = self.ensure_entered(ctx);
        let handled = {
            let mut cx = Cx {
                ctx,
                out: &mut self.out,
                schematic: self.schematic,
                settings: &self.settings,
            };
            self.states.get(kind).is_some_and(|s| s.abort(&mut cx))
        };
        let result = handled || self.set_next_state(ctx, StateKind::Select);
        self.after_event(ctx);
        result
    }

    /// Selects all items.
    pub fn select_all(&mut self, ctx: &mut SchematicContext<'_>) -> bool {
        dispatch!(self, ctx, |s, cx| s.select_all(&mut cx))
    }

    /// Cuts the selected items to the clipboard.
    pub fn cut(&mut self, ctx: &mut SchematicContext<'_>) -> bool {
        dispatch!(self, ctx, |s, cx| s.cut(&mut cx))
    }

    /// Copies the selected items to the clipboard.
    pub fn copy(&mut self, ctx: &mut SchematicContext<'_>) -> bool {
        dispatch!(self, ctx, |s, cx| s.copy(&mut cx))
    }

    /// Pastes items from the clipboard; they follow the cursor until the
    /// next click.
    pub fn paste(&mut self, ctx: &mut SchematicContext<'_>) -> bool {
        dispatch!(self, ctx, |s, cx| s.paste(&mut cx))
    }

    /// Moves the selected items (arrow keys).
    pub fn move_by(&mut self, ctx: &mut SchematicContext<'_>, delta: Point) -> bool {
        dispatch!(self, ctx, |s, cx| s.move_by(&mut cx, delta))
    }

    /// Rotates the selected items or the item being placed.
    pub fn rotate(&mut self, ctx: &mut SchematicContext<'_>, angle: Angle) -> bool {
        dispatch!(self, ctx, |s, cx| s.rotate(&mut cx, angle))
    }

    /// Mirrors the selected items or the item being placed.
    pub fn mirror(&mut self, ctx: &mut SchematicContext<'_>, orientation: Orientation) -> bool {
        dispatch!(self, ctx, |s, cx| s.mirror(&mut cx, orientation))
    }

    /// Snaps the selected items to the grid.
    pub fn snap_to_grid(&mut self, ctx: &mut SchematicContext<'_>) -> bool {
        dispatch!(self, ctx, |s, cx| s.snap_to_grid(&mut cx))
    }

    /// Resets the texts of the selected symbols.
    pub fn reset_all_texts(&mut self, ctx: &mut SchematicContext<'_>) -> bool {
        dispatch!(self, ctx, |s, cx| s.reset_all_texts(&mut cx))
    }

    /// Removes the selected items.
    pub fn remove(&mut self, ctx: &mut SchematicContext<'_>) -> bool {
        dispatch!(self, ctx, |s, cx| s.remove(&mut cx))
    }

    /// Requests the properties dialog of the selected item.
    pub fn edit_properties(&mut self, ctx: &mut SchematicContext<'_>) -> bool {
        dispatch!(self, ctx, |s, cx| s.edit_properties(&mut cx))
    }

    // --- Input events ---

    /// A key was pressed.
    pub fn key_pressed(&mut self, ctx: &mut SchematicContext<'_>, e: KeyEvent) -> bool {
        dispatch!(self, ctx, |s, cx| s.key_pressed(&mut cx, e))
    }

    /// A key was released.
    pub fn key_released(&mut self, ctx: &mut SchematicContext<'_>, e: KeyEvent) -> bool {
        dispatch!(self, ctx, |s, cx| s.key_released(&mut cx, e))
    }

    /// The pointer moved.
    pub fn pointer_moved(&mut self, ctx: &mut SchematicContext<'_>, e: PointerEvent) -> bool {
        self.out.last_pos = Some(e.pos);
        dispatch!(self, ctx, |s, cx| s.pointer_moved(&mut cx, e))
    }

    /// The left button was pressed.
    pub fn left_pressed(&mut self, ctx: &mut SchematicContext<'_>, e: PointerEvent) -> bool {
        self.out.last_pos = Some(e.pos);
        dispatch!(self, ctx, |s, cx| s.left_pressed(&mut cx, e))
    }

    /// The left button was released.
    pub fn left_released(&mut self, ctx: &mut SchematicContext<'_>, e: PointerEvent) -> bool {
        self.out.last_pos = Some(e.pos);
        dispatch!(self, ctx, |s, cx| s.left_released(&mut cx, e))
    }

    /// The left button was double-clicked.
    pub fn left_double_clicked(&mut self, ctx: &mut SchematicContext<'_>, e: PointerEvent) -> bool {
        self.out.last_pos = Some(e.pos);
        dispatch!(self, ctx, |s, cx| s.left_double_clicked(&mut cx, e))
    }

    /// The right button was released: handled by the state (e.g. rotate
    /// while placing), otherwise aborts the tool, or in the select state
    /// switches back to the previous tool.
    pub fn right_released(&mut self, ctx: &mut SchematicContext<'_>, e: PointerEvent) -> bool {
        self.out.last_pos = Some(e.pos);
        let kind = self.ensure_entered(ctx);
        let handled = {
            let mut cx = Cx {
                ctx,
                out: &mut self.out,
                schematic: self.schematic,
                settings: &self.settings,
            };
            self.states
                .get(kind)
                .is_some_and(|s| s.right_released(&mut cx, e))
        };
        let result = if handled {
            true
        } else if kind != StateKind::Select {
            self.after_event(ctx);
            return self.abort(ctx);
        } else {
            self.switch_to_previous_state(ctx)
        };
        self.after_event(ctx);
        result
    }

    // --- Tool bar ---

    /// Sets the wire mode (draw wire tool).
    pub fn set_wire_mode(&mut self, ctx: &mut SchematicContext<'_>, mode: WireMode) {
        let kind = self.ensure_entered(ctx);
        self.out.tool_data.wire_mode = mode;
        if kind == StateKind::DrawWire {
            let mut cx = Cx {
                ctx,
                out: &mut self.out,
                schematic: self.schematic,
                settings: &self.settings,
            };
            self.states.draw_wire.wire_mode_changed(&mut cx);
        }
        self.after_event(ctx);
    }

    /// Sets the layer (draw polygon, add text).
    pub fn set_layer(&mut self, ctx: &mut SchematicContext<'_>, layer: Layer) {
        let kind = self.ensure_entered(ctx);
        self.out.tool_data.layer = layer;
        self.properties_changed(ctx, kind);
    }

    /// Sets the line width (draw polygon).
    pub fn set_line_width(&mut self, ctx: &mut SchematicContext<'_>, width: UnsignedLength) {
        let kind = self.ensure_entered(ctx);
        self.out.tool_data.line_width = width;
        self.properties_changed(ctx, kind);
    }

    /// Sets whether polygons are filled (draw polygon).
    pub fn set_filled(&mut self, ctx: &mut SchematicContext<'_>, filled: bool) {
        let kind = self.ensure_entered(ctx);
        self.out.tool_data.filled = filled;
        self.properties_changed(ctx, kind);
    }

    /// Sets the text height (add text).
    pub fn set_size(&mut self, ctx: &mut SchematicContext<'_>, size: PositiveLength) {
        let kind = self.ensure_entered(ctx);
        self.out.tool_data.size = size;
        self.properties_changed(ctx, kind);
    }

    /// Sets the text (add text) or the value (add component).
    pub fn set_value(&mut self, ctx: &mut SchematicContext<'_>, value: impl Into<String>) {
        let kind = self.ensure_entered(ctx);
        self.out.tool_data.value = value.into();
        self.properties_changed(ctx, kind);
    }

    fn properties_changed(&mut self, ctx: &mut SchematicContext<'_>, kind: StateKind) {
        {
            let mut cx = Cx {
                ctx,
                out: &mut self.out,
                schematic: self.schematic,
                settings: &self.settings,
            };
            match kind {
                StateKind::DrawPolygon => self.states.draw_polygon.properties_changed(&mut cx),
                StateKind::AddText => self.states.add_text.properties_changed(&mut cx),
                StateKind::AddComponent => self.states.add_component.value_changed(&mut cx),
                _ => {}
            }
        }
        self.after_event(ctx);
    }

    // --- State handling ---

    fn switch_to(&mut self, ctx: &mut SchematicContext<'_>, kind: StateKind) -> bool {
        self.ensure_entered(ctx);
        let ok = self.set_next_state(ctx, kind);
        self.after_event(ctx);
        ok
    }

    /// Enters the initial state on first use (it needs a context).
    fn ensure_entered(&mut self, ctx: &mut SchematicContext<'_>) -> StateKind {
        if !self.entered {
            self.entered = true;
            let kind = self.current;
            self.current = StateKind::Idle;
            if !self.enter_next_state(ctx, kind) {
                self.enter_next_state(ctx, StateKind::Select);
            }
        }
        self.current
    }

    fn set_next_state(&mut self, ctx: &mut SchematicContext<'_>, kind: StateKind) -> bool {
        if kind == self.current {
            return true;
        }
        if !self.leave_current_state(ctx) {
            return false;
        }
        if !self.enter_next_state(ctx, kind) {
            self.enter_next_state(ctx, StateKind::Select);
            return false;
        }
        true
    }

    fn leave_current_state(&mut self, ctx: &mut SchematicContext<'_>) -> bool {
        let kind = self.current;
        let ok = {
            let mut cx = Cx {
                ctx,
                out: &mut self.out,
                schematic: self.schematic,
                settings: &self.settings,
            };
            self.states.get(kind).is_none_or(|s| s.exit(&mut cx))
        };
        if !ok {
            return false;
        }
        // Only memorize states other than select and add component.
        if !matches!(
            kind,
            StateKind::Select | StateKind::AddComponent | StateKind::Idle
        ) {
            self.previous = kind;
        }
        self.current = StateKind::Idle;
        self.out.tool = SchematicTool::Select;
        self.out.view.cursor = None;
        self.out.view.scene_cursor = None;
        self.out.view.rubber_band = None;
        self.out.view.ruler = None;
        self.out.view.gray_out = false;
        self.out.view.info_box.clear();
        self.out.view.features = Default::default();
        self.out.hovered = None;
        true
    }

    fn enter_next_state(&mut self, ctx: &mut SchematicContext<'_>, kind: StateKind) -> bool {
        let ok = {
            let mut cx = Cx {
                ctx,
                out: &mut self.out,
                schematic: self.schematic,
                settings: &self.settings,
            };
            self.states.get(kind).is_none_or(|s| s.entry(&mut cx))
        };
        if ok {
            self.current = kind;
        }
        ok
    }

    fn switch_to_previous_state(&mut self, ctx: &mut SchematicContext<'_>) -> bool {
        let mut next = self.previous;
        if next == self.current || next == StateKind::Idle {
            next = StateKind::Select;
        }
        self.set_next_state(ctx, next)
    }

    /// Processes queued state requests and lets the state update its
    /// outputs.
    fn after_event(&mut self, ctx: &mut SchematicContext<'_>) {
        if std::mem::take(&mut self.out.leave_requested) {
            self.set_next_state(ctx, StateKind::Select);
        }
        // Forget removed items (e.g. after undo).
        if let Some(s) = ctx.editor.project().schematic(self.schematic) {
            self.out
                .selection
                .retain(|item| selection::item_exists(item, s));
            if self
                .out
                .hovered
                .is_some_and(|item| !selection::item_exists(&item, s))
            {
                self.out.hovered = None;
            }
        }
        let kind = self.current;
        let mut cx = Cx {
            ctx,
            out: &mut self.out,
            schematic: self.schematic,
            settings: &self.settings,
        };
        if let Some(state) = self.states.get(kind) {
            state.update(&mut cx);
        }
    }
}

static_assertions::assert_impl_all!(SchematicEditorFsm: Send, Sync);
