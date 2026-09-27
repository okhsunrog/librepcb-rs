//! Outputs of the board editor FSM: what the view shows (cursor, overlays,
//! info box), the tool bar data (upstream `Board2dTabData::tool-*` and
//! `BoardEditorFsmAdapter::Features`) and events for the application
//! (dialogs, clipboard, messages).

use std::collections::BTreeSet;

use bitflags::bitflags;
use librepcb_core::geometry::ZoneRules;
use librepcb_core::project::{ComponentInstanceId, NetSignalId};
use librepcb_core::types::{Angle, Layer, Point, PositiveLength, UnsignedLength, Uuid};

use super::clipboard::ClipboardContent;
use super::input::{CursorShape, SceneCursor, StatusMessage};
use super::view::BoardItemRef;

/// The tools (states) of the board editor (upstream
/// `BoardEditorFsm::State`, UI `EditorTool`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum BoardTool {
    /// Select, move and modify items.
    #[default]
    Select,
    /// Draw traces.
    DrawTrace,
    /// Add vias.
    AddVia,
    /// Draw polygons.
    DrawPolygon,
    /// Draw planes.
    DrawPlane,
    /// Draw keepout zones.
    DrawZone,
    /// Add non-plated holes.
    AddHole,
    /// Add stroke texts.
    AddStrokeText,
    /// Place a device (started with [`BoardFsmInput::AddDevice`](super::BoardFsmInput::AddDevice)).
    AddDevice,
    /// Measure distances.
    Measure,
}

/// The wire mode of the trace tool (upstream
/// `BoardEditorState_DrawTrace::WireMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum WireMode {
    /// Horizontal, then vertical.
    #[default]
    HV,
    /// Vertical, then horizontal.
    VH,
    /// 90°, then 45°.
    Deg9045,
    /// 45°, then 90°.
    Deg4590,
    /// Straight.
    Straight,
}

impl WireMode {
    /// The next mode (cycled with the right mouse button).
    pub fn next(self) -> Self {
        match self {
            Self::HV => Self::VH,
            Self::VH => Self::Deg9045,
            Self::Deg9045 => Self::Deg4590,
            Self::Deg4590 => Self::Straight,
            Self::Straight => Self::HV,
        }
    }
}

/// The net selection of a tool (upstream: "[Auto]", "[None]" or a net).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ToolNet {
    /// Whether the net is determined automatically (vias only).
    pub auto: bool,
    /// The net (`None`: no net).
    pub net: Option<NetSignalId>,
}

/// Tool bar values of the active tool (upstream `Board2dTabData`
/// `tool-*` fields). Fields which the active tool does not use keep their
/// defaults.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BoardToolData {
    /// The active tool.
    pub tool: BoardTool,
    /// Wire mode (trace tool).
    pub wire_mode: WireMode,
    /// Nets to choose from, sorted by name (via, plane tools).
    pub nets: Vec<(NetSignalId, String)>,
    /// Current net (via, plane, trace tools).
    pub net: ToolNet,
    /// Name of the net class of the current net (read-only; empty if
    /// none).
    pub net_class_name: String,
    /// Layers to choose from, sorted.
    pub layers: Vec<Layer>,
    /// Current layer.
    pub layer: Option<Layer>,
    /// Trace width, polygon line width.
    pub line_width: Option<UnsignedLength>,
    /// Whether the trace width is automatic (UI: `tool-filled`).
    pub auto_width: bool,
    /// Via size or text height (UI: `tool-size`).
    pub size: Option<PositiveLength>,
    /// Whether the via size is automatic (UI: `tool-mirrored`).
    pub auto_size: bool,
    /// Via drill or hole diameter (UI: `tool-drill`).
    pub drill: Option<PositiveLength>,
    /// Whether the via drill is automatic (UI: `tool-pressfit`).
    pub auto_drill: bool,
    /// Arc angle of the next segment (polygon, zone tools).
    pub angle: Angle,
    /// Whether polygons are filled.
    pub filled: bool,
    /// Whether texts are mirrored.
    pub mirrored: bool,
    /// Text of the text tool.
    pub value: String,
    /// Suggestions for the text.
    pub value_suggestions: Vec<String>,
    /// Zone rules (zone tool).
    pub zone_rules: ZoneRules,
}

/// Changes of tool bar values by the user (upstream `Board2dTab` signals
/// `*Requested`).
#[derive(Debug, Clone, PartialEq)]
pub enum ToolSetting {
    /// Wire mode.
    WireMode(WireMode),
    /// Net.
    Net(ToolNet),
    /// Layer (zone tool: the only layer of new zones).
    Layer(Layer),
    /// Trace width.
    TraceWidth(PositiveLength),
    /// Polygon line width.
    LineWidth(UnsignedLength),
    /// Automatic trace width.
    AutoWidth(bool),
    /// Via size (`None`: automatic).
    ViaSize(Option<PositiveLength>),
    /// Via drill (`None`: automatic).
    ViaDrill(Option<PositiveLength>),
    /// Hole diameter.
    HoleDiameter(PositiveLength),
    /// Text height.
    TextHeight(PositiveLength),
    /// Text.
    Text(String),
    /// Mirrored text.
    Mirrored(bool),
    /// Filled polygon.
    Filled(bool),
    /// Arc angle.
    Angle(Angle),
    /// Zone rules.
    ZoneRules(ZoneRules),
    /// Store the trace width as board default.
    SaveTraceWidthInBoard,
    /// Store the trace width as net class default.
    SaveTraceWidthInNetClass,
    /// Store the via drill as board default.
    SaveViaDrillInBoard,
    /// Store the via drill as net class default.
    SaveViaDrillInNetClass,
}

bitflags! {
    /// Actions available in the current state (upstream
    /// `BoardEditorFsmAdapter::Feature`).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct BoardFeatures: u32 {
        /// Select (all).
        const SELECT = 1 << 0;
        /// Cut.
        const CUT = 1 << 1;
        /// Copy.
        const COPY = 1 << 2;
        /// Paste.
        const PASTE = 1 << 3;
        /// Remove.
        const REMOVE = 1 << 4;
        /// Rotate.
        const ROTATE = 1 << 5;
        /// Flip.
        const FLIP = 1 << 6;
        /// Snap to grid.
        const SNAP_TO_GRID = 1 << 8;
        /// Reset texts.
        const RESET_TEXTS = 1 << 9;
        /// Lock.
        const LOCK = 1 << 10;
        /// Unlock.
        const UNLOCK = 1 << 11;
        /// Properties.
        const PROPERTIES = 1 << 12;
        /// Modify line width.
        const MODIFY_LINE_WIDTH = 1 << 13;
        /// Import graphics.
        const IMPORT_GRAPHICS = 1 << 14;
    }
}

/// An action of a context menu entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContextAction {
    /// Open the properties dialog.
    Properties,
    /// Rotate counter-clockwise.
    RotateCcw,
    /// Rotate clockwise.
    RotateCw,
    /// Flip horizontally.
    FlipHorizontal,
    /// Flip vertically.
    FlipVertical,
    /// Remove the selection.
    Remove,
    /// Cut.
    Cut,
    /// Copy.
    Copy,
    /// Snap to grid.
    SnapToGrid,
    /// Lock (`true`) or unlock.
    Lock(bool),
    /// Reset all texts of the device.
    ResetTexts,
    /// Change the footprint of the device.
    ChangeFootprint(Uuid),
    /// Change the 3D model of the device.
    ChangeModel(Option<Uuid>),
    /// Set the width of the selected traces.
    SetLineWidth,
    /// Remove the whole trace (net segment).
    RemoveWholeTrace,
    /// Select the whole trace (net segment).
    SelectWholeTrace,
    /// Measure the length of the selected traces.
    MeasureLength,
    /// Remove the vertices at the clicked position.
    RemoveVertices,
    /// Add a vertex at the clicked position.
    AddVertex,
    /// Show or hide the plane.
    PlaneVisible(bool),
}

/// An entry of a context menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextMenuItem {
    /// The action (`None`: separator).
    pub action: Option<ContextAction>,
    /// The (translated) text.
    pub text: String,
    /// Whether the entry is enabled.
    pub enabled: bool,
    /// Check state of checkable entries.
    pub checked: Option<bool>,
    /// Whether this is the default action.
    pub default: bool,
}

/// An event for the application (upstream: dialogs, message boxes,
/// clipboard access and status bar messages).
#[derive(Debug, Clone, PartialEq)]
pub enum BoardFsmEvent {
    /// Set the status bar message.
    StatusMessage(StatusMessage),
    /// Show an error (upstream `QMessageBox::critical()`).
    Error(String),
    /// Show a message (e.g. a measurement result).
    Message {
        /// Title.
        title: String,
        /// Text.
        text: String,
        /// Whether it is a warning.
        warning: bool,
    },
    /// Open the properties dialog of an item.
    OpenProperties(BoardItemRef),
    /// Put data on the clipboard.
    SetClipboard(ClipboardContent),
    /// Put text on the clipboard.
    SetClipboardText(String),
    /// Show a context menu at a scene position; the chosen entry is
    /// passed back as [`BoardFsmInput::ContextMenu`](super::BoardFsmInput::ContextMenu).
    ContextMenu {
        /// Position in the scene.
        pos: Point,
        /// The entries.
        items: Vec<ContextMenuItem>,
    },
    /// Ask for a line width (upstream "Set Width" dialog); pass the value
    /// back as [`BoardFsmInput::SetLineWidth`](super::BoardFsmInput::SetLineWidth).
    RequestLineWidth {
        /// The current (median) width.
        current: UnsignedLength,
    },
    /// Components placed on the board changed (refresh the unplaced
    /// components list).
    DevicesChanged(BTreeSet<ComponentInstanceId>),
}

/// The state of the view the FSM controls (polled by the application).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BoardFsmOutput {
    /// The mouse cursor.
    pub cursor: CursorShape,
    /// Text of the info box overlay (upstream `fsmSetViewInfoBoxText()`,
    /// plain text with line breaks).
    pub info_box: String,
    /// The ruler of the measure tool.
    pub ruler: Option<(Point, Point)>,
    /// The scene cursor (measure tool).
    pub scene_cursor: Option<SceneCursor>,
    /// The selection rectangle while selecting with the rubber band.
    pub selection_rect: Option<(Point, Point)>,
    /// Whether the scene is grayed out (measure tool).
    pub gray_out: bool,
    /// Nets to highlight (upstream cross probing).
    pub highlighted_nets: BTreeSet<NetSignalId>,
    /// The item under the cursor (select tool, idle).
    pub hover: Option<BoardItemRef>,
    /// Available actions.
    pub features: BoardFeatures,
    /// Tool bar data.
    pub tool: BoardToolData,
    /// Events since the last [`take_events()`](super::BoardEditorFsm::take_events).
    pub events: Vec<BoardFsmEvent>,
}
