//! Port of libs/librepcb/editor/project/schematic/fsm/schematiceditorstate_drawwire.{h,cpp}.
//!
//! The wire is built with the [`DrawWire`] command (which ports the model
//! changes of upstream's `startPositioning()`/`addNextNetPoint()`): while
//! positioning, the group of the current segment contains the preview
//! (from the fixed start anchor over the corner to the cursor, never
//! connected at the cursor side); a click replaces it by the final wire
//! connected to the anchor under the cursor and commits the group.
//!
//! Differences to upstream: buses are not supported (no bus junction
//! anchors, no net label menu); forced net names of pins are applied by
//! the command.

use std::collections::BTreeSet;

use librepcb_core::project::NetSegmentId;
use librepcb_core::types::Point;
use librepcb_core::utils::toolbox;
use librepcb_i18n::tr;

use super::hit_test::{FindFlags, find_items_at, pin_position};
use super::simplify::SimplifySchematicSegments;
use super::{Cx, SchematicItem, SchematicTool, State};
use crate::commands::{DrawWire, WireAnchor, WireMode, WireResult};
use crate::fsm::{CursorShape, Key, KeyEvent, PointerEvent};

/// The draw wire state.
#[derive(Debug, Default)]
pub(crate) struct DrawWireState {
    /// Positioning: the fixed start anchor and its position.
    start: Option<(WireAnchor, Point)>,
    /// The last preview.
    preview: Option<WireResult>,
    /// The segment of the last committed wire part (for the simplification
    /// when finishing).
    segments: BTreeSet<NetSegmentId>,
    cursor: Point,
}

/// The flags upstream's `findItem()` uses: only anchors.
fn anchor_flags() -> FindFlags {
    FindFlags {
        net_points: true,
        net_lines: true,
        symbol_pins: true,
        ..FindFlags::NONE
    }
    .within_grid()
}

impl DrawWireState {
    fn is_positioning(&self) -> bool {
        self.start.is_some()
    }

    /// Returns the item to snap to and the snapped position (upstream
    /// `updateNetpointPositions()` item lookup).
    fn snap_target(&self, cx: &Cx<'_, '_>, snap: bool) -> (Option<SchematicItem>, Point) {
        let mut pos = self.cursor.mapped_to_grid(cx.grid());
        if !snap {
            return (None, pos);
        }
        let Some(s) = cx.sch() else {
            return (None, pos);
        };
        let item = find_items_at(cx, self.cursor, anchor_flags(), &[])
            .into_iter()
            .next();
        match item {
            Some(SchematicItem::NetPoint(seg, j)) => {
                pos = s.net_segments()[&seg].junctions()[&j].position();
            }
            Some(SchematicItem::SymbolPin(symbol, pin)) => {
                if let Some(p) = pin_position(cx, s, symbol, pin) {
                    pos = p;
                }
            }
            Some(SchematicItem::NetLine(seg, l)) => {
                let segment = &s.net_segments()[&seg];
                let line = &segment.lines()[&l];
                let ap = |a| s.net_line_anchor_position(seg, a, cx.project().view());
                if let (Some(p1), Some(p2)) = (ap(line.p1()), ap(line.p2())) {
                    pos = toolbox::nearest_point_on_line(pos, p1, p2);
                }
            }
            _ => {}
        }
        (item, pos)
    }

    /// Converts a hit item to a wire anchor.
    fn anchor_of(item: Option<SchematicItem>, pos: Point) -> WireAnchor {
        match item {
            Some(SchematicItem::NetPoint(segment, junction)) => {
                WireAnchor::Junction { segment, junction }
            }
            Some(SchematicItem::SymbolPin(symbol, pin)) => WireAnchor::SymbolPin { symbol, pin },
            Some(SchematicItem::NetLine(segment, line)) => WireAnchor::Line {
                segment,
                line,
                position: pos,
            },
            _ => WireAnchor::Point(pos),
        }
    }

    /// The corner points between the start and `pos`.
    fn corner(cx: &Cx<'_, '_>, start: Point, pos: Point) -> Vec<Point> {
        let middle = cx.out.tool_data.wire_mode.middle_point(start, pos);
        if middle != start && middle != pos {
            vec![middle]
        } else {
            Vec::new()
        }
    }

    /// Replaces the preview (upstream `updateNetpointPositions()`).
    fn update_preview(&mut self, cx: &mut Cx<'_, '_>, snap: bool) -> Point {
        cx.ctx.editor.rollback_group_to(0);
        self.preview = None;
        let (_, pos) = self.snap_target(cx, snap);
        let Some((start, start_pos)) = self.start.clone() else {
            return pos;
        };
        cx.out.view.scene_cursor = None;
        if pos != start_pos {
            let points = Self::corner(cx, start_pos, pos);
            match cx.ctx.editor.execute(DrawWire {
                schematic: Some(cx.schematic),
                start,
                end: WireAnchor::Point(pos),
                points,
                net: None,
            }) {
                Ok(result) => self.preview = Some(result),
                Err(e) => log::debug!("Wire preview failed: {e}"),
            }
        }
        pos
    }

    /// Starts drawing at the cursor (upstream `startPositioning()`).
    fn start_positioning(&mut self, cx: &mut Cx<'_, '_>, snap: bool) -> bool {
        let (item, pos) = self.snap_target(cx, snap);
        let anchor = Self::anchor_of(item, pos);
        if let Err(e) = cx
            .ctx
            .editor
            .begin_group(tr!("SchematicEditorState_DrawWire", "Draw Wire"))
        {
            cx.error(e);
            return false;
        }
        self.start = Some((anchor, pos));
        self.preview = None;
        self.update_preview(cx, snap);
        true
    }

    /// Fixes the current point and continues or finishes (upstream
    /// `addNextNetPoint()`).
    fn add_next_point(&mut self, cx: &mut Cx<'_, '_>, snap: bool) -> bool {
        let Some((start, start_pos)) = self.start.clone() else {
            return false;
        };
        cx.ctx.editor.rollback_group_to(0);
        self.preview = None;
        let (item, pos) = self.snap_target(cx, snap);
        // Abort if p2 == p0 (no line drawn).
        if pos == start_pos {
            self.abort_positioning(cx, true);
            return false;
        }
        let end = Self::anchor_of(item, pos);
        let finish = !matches!(end, WireAnchor::Point(_));
        let points = Self::corner(cx, start_pos, pos);
        let result = cx.ctx.editor.execute(DrawWire {
            schematic: Some(cx.schematic),
            start,
            end,
            points,
            net: None,
        });
        let result = match result {
            Ok(result) => result,
            Err(e) => {
                cx.error(e);
                self.abort_positioning(cx, true);
                return false;
            }
        };
        if let Err(e) = cx.ctx.editor.commit_group() {
            cx.error(e);
        }
        self.segments.insert(result.segment);
        self.start = None;
        if finish {
            self.abort_positioning(cx, true);
            return false;
        }
        // Continue from the new end junction.
        let junction = cx.sch().and_then(|s| {
            let seg = s.net_segments().get(&result.segment)?;
            result
                .junctions
                .iter()
                .find(|j| seg.junctions().get(*j).is_some_and(|x| x.position() == pos))
                .copied()
        });
        let Some(junction) = junction else {
            self.abort_positioning(cx, true);
            return false;
        };
        if let Err(e) = cx
            .ctx
            .editor
            .begin_group(tr!("SchematicEditorState_DrawWire", "Draw Wire"))
        {
            cx.error(e);
            return false;
        }
        self.start = Some((
            WireAnchor::Junction {
                segment: result.segment,
                junction,
            },
            pos,
        ));
        self.update_preview(cx, snap);
        true
    }

    /// Stops drawing (upstream `abortPositioning()`): discards the preview
    /// and simplifies the drawn segments.
    fn abort_positioning(&mut self, cx: &mut Cx<'_, '_>, simplify: bool) -> bool {
        let mut ok = true;
        self.start = None;
        self.preview = None;
        if cx.ctx.editor.undo_stack().is_group_active()
            && let Err(e) = cx.ctx.editor.abort_group()
        {
            cx.error(e);
            ok = false;
        }
        let segments = std::mem::take(&mut self.segments);
        if simplify && !segments.is_empty() {
            let segments: BTreeSet<NetSegmentId> = segments
                .into_iter()
                .filter(|s| cx.sch().is_some_and(|x| x.net_segments().contains_key(s)))
                .collect();
            if let Err(e) = cx.ctx.editor.execute(SimplifySchematicSegments {
                schematic: cx.schematic,
                segments,
            }) {
                log::error!("Failed to simplify net segments: {e}");
            }
        }
        ok
    }

    /// The wire mode was changed from the tool bar (upstream
    /// `setWireMode()`).
    pub fn wire_mode_changed(&mut self, cx: &mut Cx<'_, '_>) {
        if self.is_positioning() {
            self.update_preview(cx, true);
        }
    }
}

impl State for DrawWireState {
    fn entry(&mut self, cx: &mut Cx<'_, '_>) -> bool {
        cx.out.tool = SchematicTool::Wire;
        cx.set_cursor(Some(CursorShape::Cross));
        true
    }

    fn exit(&mut self, cx: &mut Cx<'_, '_>) -> bool {
        if self.is_positioning() {
            self.abort_positioning(cx, true);
        }
        cx.set_cursor(None);
        true
    }

    fn abort(&mut self, cx: &mut Cx<'_, '_>) -> bool {
        if self.is_positioning() {
            return self.abort_positioning(cx, true);
        }
        false
    }

    fn key_pressed(&mut self, cx: &mut Cx<'_, '_>, e: KeyEvent) -> bool {
        if e.key == Key::Shift && self.is_positioning() {
            self.update_preview(cx, false);
            return true;
        }
        false
    }

    fn key_released(&mut self, cx: &mut Cx<'_, '_>, e: KeyEvent) -> bool {
        if e.key == Key::Shift && self.is_positioning() {
            self.update_preview(cx, true);
            return true;
        }
        false
    }

    fn pointer_moved(&mut self, cx: &mut Cx<'_, '_>, e: PointerEvent) -> bool {
        self.cursor = e.pos;
        if self.is_positioning() {
            self.update_preview(cx, !e.modifiers.shift);
            return true;
        }
        false
    }

    fn left_pressed(&mut self, cx: &mut Cx<'_, '_>, e: PointerEvent) -> bool {
        self.cursor = e.pos;
        let snap = !e.modifiers.shift;
        if self.is_positioning() {
            self.add_next_point(cx, snap)
        } else {
            self.start_positioning(cx, snap)
        }
    }

    fn left_double_clicked(&mut self, cx: &mut Cx<'_, '_>, e: PointerEvent) -> bool {
        self.cursor = e.pos;
        if self.is_positioning() {
            return self.add_next_point(cx, !e.modifiers.shift);
        }
        false
    }

    fn right_released(&mut self, cx: &mut Cx<'_, '_>, e: PointerEvent) -> bool {
        self.cursor = e.pos;
        if self.is_positioning() {
            let mode = &mut cx.out.tool_data.wire_mode;
            *mode = match *mode {
                WireMode::HV => WireMode::VH,
                WireMode::VH => WireMode::Deg9045,
                WireMode::Deg9045 => WireMode::Deg4590,
                WireMode::Deg4590 => WireMode::Straight,
                WireMode::Straight => WireMode::HV,
            };
            self.update_preview(cx, true);
            // Always accept the event while drawing (otherwise the FSM
            // aborts the tool).
            return true;
        }
        false
    }
}
