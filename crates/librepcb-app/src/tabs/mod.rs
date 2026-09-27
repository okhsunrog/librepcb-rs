//! Window tabs.
//!
//! Port of libs/librepcb/editor/windowtab.{h,cpp}: the base behavior of all
//! tabs (UI data, actions, scene rendering and events). Upstream uses a
//! class hierarchy; here [`Tab`] is an enum over the tab kinds, which keeps
//! the per-kind data (`SchematicTabData`, `Board2dTabData`, ...) in the tab
//! types.
//!
//! Ported tab kinds: [`HomeTab`](home), [`SchematicTab`] and
//! [`Board2dTab`]. Library editor tabs, the 3D view and the project library
//! tab follow in later milestones (their `.slint` files are already
//! there).

pub mod board_2d;
pub mod board_view;
pub mod editing;
pub mod home;
pub mod schematic;
pub mod schematic_view;

use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

use librepcb_app_ui as ui;
use librepcb_canvas::kurbo::Point;
use librepcb_canvas::{Modifiers, PointerButton, PointerKind};
use std::collections::BTreeSet;

use librepcb_core::project::{BoardId, ComponentInstanceId, NetSignalId, SchematicId, SymbolId};
use librepcb_core::types::{GridStyle, Length, LengthUnit, UnsignedLength, Uuid};
use librepcb_editor::fsm::board::BoardItemRef;
use librepcb_editor::fsm::schematic::{ComponentChoice, SchematicTool};
use slint::language::{PointerEvent, PointerEventButton, PointerEventKind};

pub use board_2d::Board2dTab;
pub use schematic::SchematicTab;

use crate::notifications::Notification;
use crate::project::AppProject;

/// A unique identifier of a tab (stable while tabs move between sections).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TabId(u64);

impl TabId {
    /// A new unique identifier.
    pub fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

impl Default for TabId {
    fn default() -> Self {
        Self::new()
    }
}

/// A window tab.
pub enum Tab {
    /// The home tab (upstream `HomeTab`).
    Home(TabId),
    /// A schematic page.
    Schematic(Box<SchematicTab>),
    /// A board (2D view).
    Board2d(Box<Board2dTab>),
}

/// What an event or action changed in a tab.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TabUpdate {
    /// The scene must be repainted (the tab's frame counter is bumped).
    pub repaint: bool,
    /// The tab's UI data changed.
    pub data_changed: bool,
    /// New cursor position (world coordinates) with the unit to show it.
    pub cursor: Option<(Point, LengthUnit)>,
    /// A status bar message (empty to clear it).
    pub status: Option<String>,
    /// The tab wants to be closed.
    pub close: bool,
    /// Requests to the application (notifications, menus, dialogs).
    pub requests: Vec<TabRequest>,
    /// The project was modified: other tabs of the project must update
    /// their scenes.
    pub project_modified: bool,
}

/// An entry of a scene context menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextMenuEntry {
    /// The (translated) text; empty for a separator.
    pub text: String,
    /// Whether the entry can be chosen.
    pub enabled: bool,
    /// Check state of checkable entries.
    pub checked: Option<bool>,
    /// Whether this is the default action (shown bold upstream).
    pub is_default: bool,
}

impl ContextMenuEntry {
    /// An enabled entry.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            enabled: true,
            checked: None,
            is_default: false,
        }
    }

    /// A separator.
    pub fn separator() -> Self {
        Self {
            text: String::new(),
            enabled: false,
            checked: None,
            is_default: false,
        }
    }

    /// Whether this is a separator.
    pub fn is_separator(&self) -> bool {
        self.text.is_empty()
    }
}

/// A request of a tab to the application (upstream: dialogs, message boxes
/// and menus the tabs open themselves).
#[derive(Debug, Clone, PartialEq)]
pub enum TabRequest {
    /// Show a notification.
    Notify(Notification),
    /// Show a context menu at a position of the scene (logical pixels,
    /// relative to the scene); the chosen entry is passed back with
    /// [`Tab::context_menu_action()`].
    ContextMenu {
        /// Position in the scene.
        pos: Point,
        /// The entries.
        entries: Vec<ContextMenuEntry>,
    },
    /// Open the "add component" chooser; the choice is passed back with
    /// [`Tab::add_component()`].
    AddComponent {
        /// Preselected search term.
        search_term: String,
    },
    /// Open the properties dialog of an item.
    Properties(PropertiesTarget),
    /// Ask for a line width (board "Set Width" dialog); the answer is
    /// passed back with [`Tab::set_line_width()`].
    LineWidth {
        /// The current width.
        current: UnsignedLength,
    },
}

/// The item of a properties dialog request.
#[derive(Debug, Clone, PartialEq)]
pub enum PropertiesTarget {
    /// A symbol (and its component).
    Symbol(SymbolId),
    /// A schematic net segment (net label rename dialog).
    NetSegment(SchematicId, Uuid),
    /// A schematic bus segment (bus label rename dialog).
    BusSegment(SchematicId, Uuid),
    /// A text of a symbol.
    SymbolText(SymbolId, Uuid),
    /// A schematic polygon.
    SchematicPolygon(SchematicId, Uuid),
    /// A schematic text.
    SchematicText(SchematicId, Uuid),
    /// A board item.
    Board(BoardId, BoardItemRef),
}

/// Cross-probing: what the current tab has selected, highlighted in the
/// other tabs of the project (upstream `ProjectEditor::getCrossProbe()`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CrossProbe {
    /// Selected components (symbols, devices).
    pub components: BTreeSet<ComponentInstanceId>,
    /// Selected or highlighted nets.
    pub nets: BTreeSet<NetSignalId>,
}

impl CrossProbe {
    /// Whether nothing is probed.
    pub fn is_empty(&self) -> bool {
        self.components.is_empty() && self.nets.is_empty()
    }
}

impl TabUpdate {
    /// Only a repaint.
    pub fn repaint() -> Self {
        Self {
            repaint: true,
            ..Self::default()
        }
    }
}

impl Tab {
    /// The unique identifier.
    pub fn id(&self) -> TabId {
        match self {
            Self::Home(id) => *id,
            Self::Schematic(t) => t.id(),
            Self::Board2d(t) => t.id(),
        }
    }

    /// The project shown by the tab, if any.
    pub fn project(&self) -> Option<&Rc<AppProject>> {
        match self {
            Self::Home(_) => None,
            Self::Schematic(t) => Some(t.project()),
            Self::Board2d(t) => Some(t.project()),
        }
    }

    /// The base UI data (`TabData`).
    pub fn ui_data(&self) -> ui::TabData {
        let mut data = match self {
            Self::Home(_) => return home::ui_data(),
            Self::Schematic(t) => t.ui_data(),
            Self::Board2d(t) => t.ui_data(),
        };
        // The graphics export (PDF) is handled by the application (see
        // `outputs.rs`).
        data.features.export_graphics = ui::FeatureState::Enabled;
        data
    }

    /// Applies base UI data written by the UI (e.g. the find term).
    pub fn set_ui_data(&mut self, _data: &ui::TabData) {}

    /// The schematic tab data (default for other tabs).
    pub fn schematic_data(&self, projects: &[Rc<AppProject>]) -> ui::SchematicTabData {
        match self {
            Self::Schematic(t) => t.derived_ui_data(projects),
            _ => ui::SchematicTabData::default(),
        }
    }

    /// The board tab data (default for other tabs).
    pub fn board_data(&self, projects: &[Rc<AppProject>]) -> ui::Board2dTabData {
        match self {
            Self::Board2d(t) => t.derived_ui_data(projects),
            _ => ui::Board2dTabData::default(),
        }
    }

    /// Handles a tab action.
    pub fn trigger(&mut self, action: ui::TabAction) -> TabUpdate {
        let update = match self {
            Self::Home(_) => TabUpdate::default(),
            Self::Schematic(t) => t.trigger(action),
            Self::Board2d(t) => t.trigger(action),
        };
        if action == ui::TabAction::Close && !matches!(self, Self::Home(_)) {
            return TabUpdate {
                close: true,
                ..update
            };
        }
        update
    }

    /// Renders the scene for `Backend.render-scene`.
    pub fn render_scene(&mut self, width: f32, height: f32, scale_factor: f32) -> slint::Image {
        match self {
            Self::Home(_) => slint::Image::default(),
            Self::Schematic(t) => t.render_scene(width, height, scale_factor),
            Self::Board2d(t) => t.render_scene(width, height, scale_factor),
        }
    }

    /// Handles `Backend.scene-pointer-event`.
    pub fn pointer_event(&mut self, pos: Point, event: &PointerEvent) -> TabUpdate {
        let (kind, button, modifiers) = convert_pointer_event(event);
        match self {
            Self::Home(_) => TabUpdate::default(),
            Self::Schematic(t) => t.pointer_event(kind, button, pos, modifiers),
            Self::Board2d(t) => t.pointer_event(kind, button, pos, modifiers),
        }
    }

    /// Handles `Backend.scene-key-pressed`; returns whether the key was
    /// handled.
    pub fn key_pressed(&mut self, event: &slint::language::KeyEvent) -> (bool, TabUpdate) {
        match self {
            Self::Home(_) => (false, TabUpdate::default()),
            Self::Schematic(t) => t.key_event(event, true),
            Self::Board2d(t) => t.key_event(event, true),
        }
    }

    /// Handles `Backend.scene-key-released`; returns whether the key was
    /// handled.
    pub fn key_released(&mut self, event: &slint::language::KeyEvent) -> (bool, TabUpdate) {
        match self {
            Self::Home(_) => (false, TabUpdate::default()),
            Self::Schematic(t) => t.key_event(event, false),
            Self::Board2d(t) => t.key_event(event, false),
        }
    }

    /// An entry of the last requested context menu was chosen.
    pub fn context_menu_action(&mut self, index: usize) -> TabUpdate {
        match self {
            Self::Home(_) => TabUpdate::default(),
            Self::Schematic(t) => t.context_menu_action(index),
            Self::Board2d(t) => t.context_menu_action(index),
        }
    }

    /// Whether the tab's FSM is adding a component.
    pub fn is_adding_component(&self) -> bool {
        matches!(self, Self::Schematic(t) if t.fsm().tool() == SchematicTool::Component)
    }

    /// Whether the tab's FSM is in its select tool.
    pub fn is_select_tool(&self) -> bool {
        matches!(self, Self::Schematic(t) if t.fsm().tool() == SchematicTool::Select)
    }

    /// A component was chosen in the "add component" dialog (`None`:
    /// canceled).
    pub fn add_component(&mut self, choice: Option<ComponentChoice>) -> TabUpdate {
        match self {
            Self::Schematic(t) => t.add_component(choice),
            _ => TabUpdate::default(),
        }
    }

    /// The answer of [`TabRequest::LineWidth`].
    pub fn set_line_width(&mut self, width: UnsignedLength) -> TabUpdate {
        match self {
            Self::Board2d(t) => t.set_line_width(width),
            _ => TabUpdate::default(),
        }
    }

    /// The length unit for dialogs opened by the tab (the grid unit).
    pub fn length_unit(&self) -> Option<LengthUnit> {
        match self {
            Self::Home(_) => None,
            Self::Schematic(t) => Some(t.length_unit()),
            Self::Board2d(t) => Some(t.length_unit()),
        }
    }

    /// Aborts a tool which keeps an undo group of the project open (before
    /// undo/redo or when another tab of the project becomes current,
    /// upstream `abortBlockingToolsInOtherEditors()`).
    pub fn abort_blocking_tool(&mut self) -> TabUpdate {
        match self {
            Self::Home(_) => TabUpdate::default(),
            Self::Schematic(t) => t.abort_blocking_tool(),
            Self::Board2d(t) => t.abort_blocking_tool(),
        }
    }

    /// What this tab cross-probes to the other tabs of its project.
    pub fn cross_probe(&self) -> Option<CrossProbe> {
        match self {
            Self::Home(_) => None,
            Self::Schematic(t) => Some(t.cross_probe()),
            Self::Board2d(t) => Some(t.cross_probe()),
        }
    }

    /// Highlights what another tab of the project cross-probes.
    pub fn set_cross_probe(&mut self, probe: &CrossProbe) -> TabUpdate {
        match self {
            Self::Home(_) => TabUpdate::default(),
            Self::Schematic(t) => t.set_cross_probe(probe),
            Self::Board2d(t) => t.set_cross_probe(probe),
        }
    }

    /// Handles `Backend.scene-scrolled`; returns whether it was handled.
    pub fn scrolled(&mut self, pos: Point, delta: (f64, f64), modifiers: Modifiers) -> bool {
        match self {
            Self::Home(_) => false,
            Self::Schematic(t) => t.scrolled(pos, delta.into(), modifiers),
            Self::Board2d(t) => t.scrolled(pos, delta.into(), modifiers),
        }
    }

    /// Applies derived schematic data written by the UI.
    pub fn set_schematic_data(&mut self, data: &ui::SchematicTabData) -> TabUpdate {
        match self {
            Self::Schematic(t) => t.set_derived_ui_data(data),
            _ => TabUpdate::default(),
        }
    }

    /// Applies derived board data written by the UI.
    pub fn set_board_data(&mut self, data: &ui::Board2dTabData) -> TabUpdate {
        match self {
            Self::Board2d(t) => t.set_derived_ui_data(data),
            _ => TabUpdate::default(),
        }
    }

    /// Applies layer data written by the UI (layers panel).
    pub fn set_layer_data(&mut self, row: usize, data: &ui::GraphicsLayerData) -> TabUpdate {
        match self {
            Self::Board2d(t) => t.set_layer_visible(row, data.visible),
            _ => TabUpdate::default(),
        }
    }

    /// Bumps the frame counter of scene tabs (the UI then requests a new
    /// image).
    pub fn bump_frame(&mut self) {
        match self {
            Self::Home(_) => {}
            Self::Schematic(t) => t.bump_frame(),
            Self::Board2d(t) => t.bump_frame(),
        }
    }

    /// Applies the schematic or board grid style (workspace settings).
    pub fn set_grid_styles(&mut self, schematic: GridStyle, board: GridStyle) -> TabUpdate {
        match self {
            Self::Home(_) => TabUpdate::default(),
            Self::Schematic(t) => t.set_grid_style(schematic),
            Self::Board2d(t) => t.set_grid_style(board),
        }
    }

    /// Called when the project changed (e.g. through MCP): rebuild scenes.
    pub fn project_modified(&mut self) -> TabUpdate {
        match self {
            Self::Home(_) => TabUpdate::default(),
            Self::Schematic(t) => t.rebuild_if_modified(),
            Self::Board2d(t) => t.rebuild_if_modified(),
        }
    }
}

/// Converts Slint's pointer event.
pub fn convert_pointer_event(e: &PointerEvent) -> (PointerKind, PointerButton, Modifiers) {
    let kind = match e.kind {
        PointerEventKind::Down => PointerKind::Down,
        PointerEventKind::Up => PointerKind::Up,
        PointerEventKind::Cancel => PointerKind::Cancel,
        _ => PointerKind::Move,
    };
    let button = match e.button {
        PointerEventButton::Left => PointerButton::Left,
        PointerEventButton::Right => PointerButton::Right,
        PointerEventButton::Middle => PointerButton::Middle,
        _ => PointerButton::Other,
    };
    (kind, button, convert_modifiers(&e.modifiers))
}

/// Converts Slint's keyboard modifiers.
pub fn convert_modifiers(m: &slint::language::KeyboardModifiers) -> Modifiers {
    Modifiers {
        shift: m.shift,
        control: m.control,
        alt: m.alt,
        meta: m.meta,
    }
}

/// The index of a project in the application's project list (for the UI
/// data), -1 if not found.
pub fn project_index(projects: &[Rc<AppProject>], project: &Rc<AppProject>) -> i32 {
    projects
        .iter()
        .position(|p| Rc::ptr_eq(p, project))
        .map_or(-1, |i| i as i32)
}

/// Formats a cursor position like upstream `MainWindow` (both coordinates
/// in the grid unit with its reasonable number of decimals).
pub fn format_cursor(pos: Point, unit: LengthUnit) -> String {
    let conv = |mm: f64| {
        Length::from_mm(mm)
            .map(|l| unit.convert_to_unit(l))
            .unwrap_or(0.0)
    };
    let decimals = unit.reasonable_number_of_decimals();
    format!(
        "{:.decimals$}, {:.decimals$}",
        conv(pos.x),
        conv(pos.y),
        decimals = decimals
    )
}

/// Upstream `toFs()`: a feature that is supported and enabled or disabled.
pub fn feature(enabled: bool) -> ui::FeatureState {
    if enabled {
        ui::FeatureState::Enabled
    } else {
        ui::FeatureState::Disabled
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_format() {
        assert_eq!(
            format_cursor(Point::new(2.54, -1.0), LengthUnit::Millimeters),
            "2.540, -1.000"
        );
    }
}
