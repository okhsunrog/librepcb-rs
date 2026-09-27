//! Input and output types of the board editor FSM which are not board
//! specific (upstream `GraphicsSceneMouseEvent`, `GraphicsSceneKeyEvent`,
//! the cursor, overlay and status bar parts of `BoardEditorFsmAdapter`).
//!
//! These are candidates for types shared with the schematic editor FSM
//! (`fsm::*`); they live here until both FSMs are unified.

use librepcb_core::types::Point;

/// Keyboard modifiers of an input event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Modifiers {
    /// Shift key (disables snapping, cycles the selection).
    pub shift: bool,
    /// Control key (toggles the selection).
    pub control: bool,
    /// Alt key.
    pub alt: bool,
    /// Meta/Super key.
    pub meta: bool,
}

impl Modifiers {
    /// No modifier.
    pub const NONE: Self = Self {
        shift: false,
        control: false,
        alt: false,
        meta: false,
    };
    /// Only Shift.
    pub const SHIFT: Self = Self {
        shift: true,
        ..Self::NONE
    };
    /// Only Control.
    pub const CONTROL: Self = Self {
        control: true,
        ..Self::NONE
    };
}

/// A key of a key event (only the keys the FSMs handle are distinguished).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    /// Shift.
    Shift,
    /// Control.
    Control,
    /// Alt.
    Alt,
    /// Any other key.
    Other,
}

/// A pointer event in scene coordinates (upstream
/// `GraphicsSceneMouseEvent`; the position where the left button was
/// pressed is tracked by the FSM).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PointerEvent {
    /// Position in the scene (board coordinates, not snapped).
    pub pos: Point,
    /// Modifiers held during the event.
    pub modifiers: Modifiers,
    /// Whether the left button is held (for move events).
    pub left_button: bool,
}

impl PointerEvent {
    /// An event at `pos` without modifiers and buttons.
    pub fn at(pos: Point) -> Self {
        Self {
            pos,
            modifiers: Modifiers::NONE,
            left_button: false,
        }
    }

    /// The same event with modifiers.
    pub fn with_modifiers(mut self, modifiers: Modifiers) -> Self {
        self.modifiers = modifiers;
        self
    }

    /// The same event with the left button held.
    pub fn with_left_button(mut self) -> Self {
        self.left_button = true;
        self
    }
}

/// The mouse cursor to show over the scene (upstream `Qt::CursorShape`
/// values the FSMs use).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum CursorShape {
    /// The default arrow.
    #[default]
    Arrow,
    /// A cross (placing and drawing tools).
    Cross,
}

/// A cross or circle drawn at a scene position (upstream
/// `fsmSetSceneCursor()`), e.g. the snapped position of the measure tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SceneCursor {
    /// The position.
    pub pos: Point,
    /// Whether a cross is drawn.
    pub cross: bool,
    /// Whether a circle is drawn (snapped to an object).
    pub circle: bool,
}

/// A status bar message (upstream `fsmSetStatusBarMessage()`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusMessage {
    /// The (translated) text; empty clears the message.
    pub text: String,
    /// How long the message is shown, `None` until replaced.
    pub timeout_ms: Option<u32>,
}
