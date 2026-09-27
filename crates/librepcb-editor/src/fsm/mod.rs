//! UI-toolkit independent editor state machines (port of
//! libs/librepcb/editor/project/{schematic,board}/fsm, see
//! `docs/ui-design.md`, decision 2).
//!
//! The application translates its pointer and key events into FSM inputs
//! and shows the FSM outputs (overlays, cursor, tool bar data, dialogs).
//!
//! - [`board`]: the board editor FSM.

pub mod board;
