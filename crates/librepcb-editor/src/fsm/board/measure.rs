//! The measure tool: port of
//! libs/librepcb/editor/project/board/fsm/boardeditorstate_measure.{h,cpp}
//! and libs/librepcb/editor/utils/measuretool.{h,cpp} (board part) with
//! the snap helpers of `EditorToolbox`.
//!
//! The cursor snaps to the grid or to the nearest snap candidate (device
//! and pad origins, pad outlines, junctions, vias, outlines of planes,
//! polygons and holes, text origins), unless Shift is held. Two clicks set
//! the start and end point of the ruler; the info box shows the distance.

use std::collections::BTreeSet;

use librepcb_core::geometry::Path;
use librepcb_core::library::pkg::Footprint;
use librepcb_core::project::Project;
use librepcb_core::project::board::Board;
use librepcb_core::types::{Angle, Length, LengthUnit, Point, PositiveLength};
use librepcb_core::utils::transform::Transform;
use librepcb_i18n::tr;

use super::context::Cx;
use super::input::{CursorShape, Key, Modifiers, SceneCursor};
use super::output::BoardToolData;
use super::{BoardFsmInput, State};

/// Upstream `EditorToolbox::snapCandidatesFromPath()`.
pub(crate) fn snap_candidates_from_path(path: &Path, out: &mut BTreeSet<Point>) {
    let vertices = path.vertices();
    for (i, v) in vertices.iter().enumerate() {
        out.insert(v.pos);
        if v.angle.abs() == Angle::DEG180
            && let Some(next) = vertices.get(i + 1)
        {
            let center = (v.pos + next.pos) / 2;
            out.insert(center);
            out.insert(v.pos.rotated(v.angle / 2, center));
        }
    }
}

/// Upstream `EditorToolbox::snapCandidatesFromCircle()`.
pub(crate) fn snap_candidates_from_circle(
    center: Point,
    diameter: PositiveLength,
    out: &mut BTreeSet<Point>,
) {
    let r = *diameter / 2;
    out.insert(center);
    out.insert(center + Point::new(Length::ZERO, r));
    out.insert(center + Point::new(Length::ZERO, -r));
    out.insert(center + Point::new(r, Length::ZERO));
    out.insert(center + Point::new(-r, Length::ZERO));
}

/// Upstream `EditorToolbox::snapPosition()`: returns the snapped position
/// and whether it snapped to a candidate.
pub(crate) fn snap_position(
    cursor: Point,
    grid: PositiveLength,
    candidates: &BTreeSet<Point>,
) -> (Point, bool) {
    if candidates.contains(&cursor) {
        return (cursor, true);
    }
    if cursor.is_on_grid(grid) {
        return (cursor, false);
    }
    let nearest = candidates
        .iter()
        .map(|c| (*(cursor - *c).length(), *c))
        .min_by_key(|(d, _)| *d);
    let on_grid = cursor.mapped_to_grid(grid);
    let grid_distance = *(cursor - on_grid).length();
    match nearest {
        Some((d, c)) if d <= grid_distance => (c, true),
        _ => (on_grid, false),
    }
}

/// Upstream `MeasureTool::snapCandidatesFromFootprint()`.
fn footprint_candidates(footprint: &Footprint, t: &Transform, out: &mut BTreeSet<Point>) {
    for pad in footprint.pads().iter() {
        let p = pad.pad();
        out.insert(t.map(&p.position()));
        if let Ok(outlines) = p.geometry().to_outlines() {
            for outline in outlines {
                let path = outline
                    .rotated(p.rotation(), Point::ORIGIN)
                    .translated(p.position());
                snap_candidates_from_path(&t.map(&path), out);
            }
        }
        let pad_t = Transform {
            position: p.position(),
            rotation: p.rotation(),
            mirrored: false,
        };
        for h in p.holes().iter() {
            for v in pad_t.map(h.path()).get().vertices() {
                snap_candidates_from_circle(t.map(&v.pos), h.diameter(), out);
            }
        }
    }
    for p in footprint.polygons().iter() {
        snap_candidates_from_path(&t.map(p.path()), out);
    }
    for c in footprint.circles().iter() {
        snap_candidates_from_circle(t.map(&c.center()), c.diameter(), out);
    }
    for s in footprint.stroke_texts().iter() {
        out.insert(t.map(&s.position()));
    }
    for h in footprint.holes().iter() {
        for v in h.path().get().vertices() {
            snap_candidates_from_circle(t.map(&v.pos), h.diameter(), out);
        }
    }
}

/// Upstream `MeasureTool::setBoard()`.
pub(crate) fn board_snap_candidates(project: &Project, board: &Board) -> BTreeSet<Point> {
    let mut out = BTreeSet::new();
    let lib = project.library();
    for d in board.devices().values() {
        out.insert(d.position());
        if let Some(footprint) = lib
            .device(&d.lib_device())
            .and_then(|dev| lib.package(&dev.package_uuid()))
            .and_then(|pkg| pkg.footprints().by_uuid(&d.lib_footprint()))
        {
            footprint_candidates(footprint, &d.transform(), &mut out);
        }
    }
    for seg in board.net_segments().values() {
        for j in seg.junctions().values() {
            out.insert(j.position());
        }
        for v in seg.vias().values() {
            out.insert(v.position());
            let props = board.via_properties(v, seg.net(), project.circuit());
            snap_candidates_from_circle(v.position(), props.size, &mut out);
            snap_candidates_from_circle(v.position(), props.drill_diameter, &mut out);
        }
    }
    for plane in board.planes().values() {
        snap_candidates_from_path(plane.outline(), &mut out);
        for fragment in board.derived().fragments_of(plane.id()) {
            snap_candidates_from_path(fragment, &mut out);
        }
    }
    for p in board.polygons().values() {
        snap_candidates_from_path(p.path(), &mut out);
    }
    for t in board.stroke_texts().values() {
        out.insert(t.position());
    }
    for h in board.holes().values() {
        for v in h.path().get().vertices() {
            snap_candidates_from_circle(v.pos, h.diameter(), &mut out);
        }
    }
    out
}

/// The measure tool (upstream `BoardEditorState_Measure` + `MeasureTool`).
#[derive(Debug, Default)]
pub(super) struct MeasureState {
    candidates: BTreeSet<Point>,
    last_pos: Point,
    cursor: Point,
    snapped: bool,
    start: Option<Point>,
    end: Option<Point>,
}

impl MeasureState {
    fn update_cursor(&mut self, cx: &mut Cx<'_, '_>, modifiers: Modifiers) {
        self.cursor = self.last_pos;
        self.snapped = false;
        if !modifiers.shift {
            (self.cursor, self.snapped) = snap_position(self.last_pos, cx.grid(), &self.candidates);
        }
        self.update_ruler(cx);
    }

    /// Upstream `updateRulerPositions()`.
    fn update_ruler(&self, cx: &mut Cx<'_, '_>) {
        cx.out.scene_cursor = Some(SceneCursor {
            pos: self.cursor,
            cross: self.start.is_none() || self.end.is_some(),
            circle: self.snapped,
        });
        let start = self.start.unwrap_or(self.cursor);
        let end = self.end.unwrap_or(self.cursor);
        cx.out.ruler = self.start.map(|_| (start, end));
        let unit = cx
            .board()
            .map(|b| b.settings().grid_unit)
            .unwrap_or(LengthUnit::Millimeters);
        let diff = end - start;
        let length = *diff.length();
        let (dx, dy) = diff.to_mm();
        let angle = libm::atan2(dy, dx).to_degrees();
        let decimals = unit.reasonable_number_of_decimals() + 1;
        let u = unit.to_short_str();
        let v = |l: Length| format!("{:>10.*}", decimals, unit.convert_to_unit(l));
        let mut text = String::new();
        text += &format!("X0: {} {u}\n", v(start.x));
        text += &format!("Y0: {} {u}\n", v(start.y));
        text += &format!("X1: {} {u}\n", v(end.x));
        text += &format!("Y1: {} {u}\n", v(end.y));
        text += "\n";
        text += &format!("ΔX: {} {u}\n", v(diff.x));
        text += &format!("ΔY: {} {u}\n", v(diff.y));
        text += "\n";
        text += &format!("Δ: {:>11.*} {u}\n", decimals, unit.convert_to_unit(length));
        text += &format!("∠: {:>width$.3}°", angle, width = 14 - decimals);
        cx.out.info_box = text;
    }

    /// Upstream `updateStatusBarMessage()`.
    fn update_status(&self, cx: &mut Cx<'_, '_>) {
        let note = format!(
            " {}",
            tr!("MeasureTool", "(press {0} to disable snap)", "Shift")
        );
        let text = if self.end.is_some() {
            tr!(
                "MeasureTool",
                "Press {0} to copy the value to clipboard or {1} to clear the measurement",
                "Ctrl+C",
                "Del"
            )
        } else if self.start.is_some() {
            tr!("MeasureTool", "Click to specify the end point") + &note
        } else {
            tr!("MeasureTool", "Click to specify the start point") + &note
        };
        cx.status(text, None);
    }
}

impl State for MeasureState {
    fn entry(&mut self, cx: &mut Cx<'_, '_>) -> bool {
        let Ok(board) = cx.board() else {
            return false;
        };
        self.candidates = board_snap_candidates(cx.project(), board);
        self.last_pos = cx.cursor_pos;
        cx.selection.clear();
        cx.out.gray_out = true;
        cx.out.cursor = CursorShape::Cross;
        self.update_cursor(cx, Modifiers::NONE);
        self.update_status(cx);
        true
    }

    fn exit(&mut self, cx: &mut Cx<'_, '_>) -> bool {
        // Keep start/end to show the ruler again when re-entering.
        cx.out.scene_cursor = None;
        cx.out.ruler = None;
        cx.out.gray_out = false;
        cx.out.info_box.clear();
        cx.out.cursor = CursorShape::Arrow;
        cx.status(String::new(), None);
        true
    }

    fn process(&mut self, cx: &mut Cx<'_, '_>, input: &BoardFsmInput) -> bool {
        match input {
            BoardFsmInput::KeyPressed(Key::Shift, m)
            | BoardFsmInput::KeyReleased(Key::Shift, m) => {
                self.update_cursor(cx, *m);
                true
            }
            BoardFsmInput::PointerMoved(e) => {
                self.last_pos = e.pos;
                self.update_cursor(cx, e.modifiers);
                true
            }
            BoardFsmInput::LeftPressed(_) => {
                if self.start.is_none() || self.end.is_some() {
                    self.start = Some(self.cursor);
                    self.end = None;
                } else {
                    self.end = Some(self.cursor);
                }
                self.update_ruler(cx);
                self.update_status(cx);
                true
            }
            BoardFsmInput::Copy => match (self.start, self.end) {
                (Some(s), Some(e)) => {
                    let unit = cx
                        .board()
                        .map(|b| b.settings().grid_unit)
                        .unwrap_or(LengthUnit::Millimeters);
                    let value = unit.convert_to_unit(*(e - s).length());
                    let text = float_to_string(value, 12);
                    cx.out
                        .events
                        .push(super::output::BoardFsmEvent::SetClipboardText(text.clone()));
                    cx.status(
                        tr!("MeasureTool", "Copied to clipboard: {0}", text),
                        Some(3000),
                    );
                    true
                }
                _ => false,
            },
            BoardFsmInput::Remove => {
                if self.start.is_some() && self.end.is_some() {
                    self.start = None;
                    self.end = None;
                    self.update_ruler(cx);
                    self.update_status(cx);
                    true
                } else {
                    false
                }
            }
            BoardFsmInput::Abort => {
                if self.start.is_some() && self.end.is_none() {
                    self.start = None;
                    self.update_ruler(cx);
                    self.update_status(cx);
                    true
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    fn tool_data(&self, _cx: &Cx<'_, '_>) -> BoardToolData {
        BoardToolData::default()
    }
}

/// Formats a number with at most `decimals` decimals, without trailing
/// zeros (upstream `Toolbox::floatToString()` in the C locale).
pub(crate) fn float_to_string(value: f64, decimals: usize) -> String {
    let s = format!("{value:.decimals$}");
    let s = if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_owned()
    } else {
        s
    };
    if s == "-0" { "0".to_owned() } else { s }
}
