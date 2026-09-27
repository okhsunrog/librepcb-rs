//! Port of libs/librepcb/editor/project/schematic/fsm/schematiceditorstate_select.{h,cpp}.
//!
//! Not ported: moving polygon vertices, resizing images, the context menu
//! itself (requested from the application), cross-probing.

use librepcb_core::project::SymbolId;
use librepcb_core::types::{Angle, Orientation, Point};
use librepcb_i18n::tr;

use super::clipboard::{
    PasteSchematicItems, SchematicClipboardData, schematic_clipboard_mime_type,
};
use super::drag::DragSelection;
use super::hit_test::{FindFlags, find_items_at};
use super::selection::{SelectionQuery, all_items, item_exists};
use super::simplify::SimplifySchematicSegments;
use super::{Cx, SchematicItem, SchematicRequest, State};
use crate::commands::{RemoveSchematicItems, SchematicSelection};
use crate::fsm::{CursorShape, Features, PointerEvent};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum SubState {
    #[default]
    Idle,
    Selecting,
    Moving,
    Pasting,
}

/// The select state.
#[derive(Debug, Default)]
pub(crate) struct SelectState {
    sub: SubState,
    start_pos: Point,
    drag: Option<DragSelection>,
    /// Length of the undo group before the drag preview (pasting: after
    /// the pasted items).
    base_len: usize,
}

impl SelectState {
    fn query(cx: &Cx<'_, '_>) -> SelectionQuery {
        match cx.sch() {
            Some(s) => SelectionQuery::new(&cx.out.selection, s),
            None => SelectionQuery::default(),
        }
    }

    /// Applies the current drag state as preview inside the open group.
    fn apply_drag_preview(&mut self, cx: &mut Cx<'_, '_>) {
        let Some(drag) = &self.drag else {
            return;
        };
        cx.ctx.editor.rollback_group_to(self.base_len);
        let mutations = drag.mutations(cx.project());
        if let Err(e) = cx.ctx.editor.execute(crate::commands::ApplyMutations {
            text: None,
            mutations,
        }) {
            log::warn!("Failed to update the drag preview: {e}");
        }
    }

    /// Starts a drag of the selection (upstream
    /// `startMovingSelectedItems()`).
    fn start_moving(&mut self, cx: &mut Cx<'_, '_>, pos: Point) -> bool {
        let query = Self::query(cx);
        let Some(s) = cx.sch() else {
            return false;
        };
        let drag = DragSelection::new(s, &query, pos);
        if let Err(e) = cx.ctx.editor.begin_group(tr!(
            "CmdDragSelectedSchematicItems",
            "Drag Schematic Elements"
        )) {
            cx.error(e);
            return false;
        }
        self.base_len = 0;
        self.drag = Some(drag);
        self.sub = SubState::Moving;
        true
    }

    /// Finishes a drag or paste: applies the final state, simplifies the
    /// modified segments and commits the group.
    fn finish_drag(&mut self, cx: &mut Cx<'_, '_>, pos: Point) {
        if let Some(drag) = &mut self.drag {
            drag.set_current_position(pos, None);
        }
        self.apply_drag_preview(cx);
        let drag = self.drag.take();
        if let Some(drag) = drag {
            let segments = drag.modified_segments().clone();
            if drag.has_changes() && !segments.is_empty() {
                let len = cx.ctx.editor.active_group_len().unwrap_or(0);
                if let Err(e) = cx.ctx.editor.execute(SimplifySchematicSegments {
                    schematic: cx.schematic,
                    segments,
                }) {
                    log::error!("Failed to simplify schematic segments: {e}");
                    cx.ctx.editor.rollback_group_to(len);
                }
            }
        }
        if let Err(e) = cx.ctx.editor.commit_group() {
            cx.error(e);
        }
        self.sub = SubState::Idle;
    }

    /// Runs a one-shot drag operation (move, rotate, mirror, snap, reset
    /// texts) as one undo group.
    fn one_shot(&mut self, cx: &mut Cx<'_, '_>, f: impl FnOnce(&mut DragSelection)) -> bool {
        let query = Self::query(cx);
        let Some(s) = cx.sch() else {
            return false;
        };
        let mut drag = DragSelection::new(s, &query, Point::ORIGIN);
        if drag.is_empty() {
            return false;
        }
        f(&mut drag);
        if !drag.has_changes() {
            return true;
        }
        let mutations = drag.mutations(cx.project());
        let segments = drag.modified_segments().clone();
        let editor = &mut cx.ctx.editor;
        let result = editor
            .begin_group(tr!(
                "CmdDragSelectedSchematicItems",
                "Drag Schematic Elements"
            ))
            .and_then(|()| {
                editor.execute(crate::commands::ApplyMutations {
                    text: None,
                    mutations,
                })
            });
        match result {
            Ok(()) => {
                if !segments.is_empty() {
                    let len = editor.active_group_len().unwrap_or(0);
                    if let Err(e) = editor.execute(SimplifySchematicSegments {
                        schematic: cx.schematic,
                        segments,
                    }) {
                        log::error!("Failed to simplify schematic segments: {e}");
                        editor.rollback_group_to(len);
                    }
                }
                if let Err(e) = editor.commit_group() {
                    cx.error(e);
                    return false;
                }
                true
            }
            Err(e) => {
                if editor.undo_stack().is_group_active() {
                    let _ = editor.abort_group();
                }
                cx.error(e);
                false
            }
        }
    }

    fn rotate_selected(&mut self, cx: &mut Cx<'_, '_>, angle: Angle) -> bool {
        let grid = cx.grid();
        if let Some(drag) = &mut self.drag {
            drag.rotate(angle, true, grid);
            self.apply_drag_preview(cx);
            return true;
        }
        self.one_shot(cx, |d| d.rotate(angle, false, grid))
    }

    fn mirror_selected(&mut self, cx: &mut Cx<'_, '_>, orientation: Orientation) -> bool {
        let grid = cx.grid();
        if let Some(drag) = &mut self.drag {
            drag.mirror(orientation, true, grid);
            self.apply_drag_preview(cx);
            return true;
        }
        self.one_shot(cx, |d| d.mirror(orientation, false, grid))
    }

    /// Removes the selected items (upstream `removeSelectedItems()`).
    fn remove_selected(&mut self, cx: &mut Cx<'_, '_>) -> bool {
        let q = Self::query(cx);
        let selection = SchematicSelection {
            symbols: q.symbols.iter().copied().collect(),
            net_lines: q.net_lines.iter().copied().collect(),
            net_labels: q.net_labels.iter().copied().collect(),
            symbol_texts: Vec::new(),
            polygons: q.polygons.iter().copied().collect(),
            texts: q.texts.iter().copied().collect(),
            images: q.images.iter().copied().collect(),
        };
        cx.out.selection.clear();
        match cx.ctx.editor.execute(RemoveSchematicItems {
            schematic: cx.schematic,
            selection,
        }) {
            Ok(_) => true,
            Err(e) => {
                cx.error(e);
                false
            }
        }
    }

    /// Copies the selection to the clipboard (upstream
    /// `copySelectedItemsToClipboard()`).
    fn copy_selected(&mut self, cx: &mut Cx<'_, '_>) -> bool {
        let query = Self::query(cx);
        let cursor = cx.cursor_pos();
        let Some(s) = cx.sch() else {
            return false;
        };
        let result = SchematicClipboardData::build(cx.project(), s, &query, cursor)
            .and_then(|data| data.to_zip());
        match result {
            Ok(zip) => {
                let mime = schematic_clipboard_mime_type(&cx.settings.app_version);
                cx.ctx.clipboard.set(&mime, zip);
                cx.out.view.set_status(
                    tr!("SchematicEditorState_Select", "Copied to clipboard!"),
                    Some(2000),
                );
            }
            Err(e) => cx.error(e),
        }
        true
    }

    /// Pastes from the clipboard (upstream `pasteFromClipboard()`).
    fn paste_from_clipboard(&mut self, cx: &mut Cx<'_, '_>) -> bool {
        let mime = schematic_clipboard_mime_type(&cx.settings.app_version);
        let Some(zip) = cx.ctx.clipboard.get(&mime) else {
            return false;
        };
        let data = match SchematicClipboardData::from_zip(&zip) {
            Ok(data) => data,
            Err(e) => {
                cx.error(e);
                return false;
            }
        };
        if data.is_empty() {
            return false;
        }
        self.start_pos = cx.cursor_pos();
        cx.out.selection.clear();
        if let Err(e) = cx.ctx.editor.begin_group(tr!(
            "SchematicEditorState_Select",
            "Paste Schematic Elements"
        )) {
            cx.error(e);
            return false;
        }
        let offset = (self.start_pos - data.cursor_pos).mapped_to_grid(cx.grid());
        match cx.ctx.editor.execute(PasteSchematicItems {
            schematic: cx.schematic,
            data,
            offset,
        }) {
            Ok(items) if !items.is_empty() => {
                cx.out.selection = items.into_iter().collect();
                self.base_len = cx.ctx.editor.active_group_len().unwrap_or(0);
                let query = Self::query(cx);
                if let Some(s) = cx.sch() {
                    self.drag = Some(DragSelection::new(s, &query, self.start_pos));
                }
                self.sub = SubState::Pasting;
                true
            }
            Ok(_) => {
                let _ = cx.ctx.editor.abort_group();
                false
            }
            Err(e) => {
                let _ = cx.ctx.editor.abort_group();
                cx.error(e);
                false
            }
        }
    }

    /// Opens the properties dialog of an item (upstream
    /// `openPropertiesDialog()`).
    fn open_properties(cx: &mut Cx<'_, '_>, item: SchematicItem) -> bool {
        let request = match item {
            SchematicItem::Symbol(id) => SchematicRequest::SymbolProperties(id),
            SchematicItem::NetLabel(seg, l) => SchematicRequest::NetLabelProperties(seg, l),
            SchematicItem::Polygon(id) => SchematicRequest::PolygonProperties(id),
            SchematicItem::Text(id) => SchematicRequest::TextProperties(id),
            _ => return false,
        };
        cx.out.requests.push(request);
        true
    }

    /// Updates the available features and the info box (upstream
    /// `updateAvailableFeatures()`).
    fn update_features(&self, cx: &mut Cx<'_, '_>) {
        let mut f = Features::default();
        match self.sub {
            SubState::Pasting => {
                f.rotate = true;
                f.mirror = true;
            }
            SubState::Idle | SubState::Moving => {
                f.select = true;
                let mime = schematic_clipboard_mime_type(&cx.settings.app_version);
                f.paste = cx.ctx.clipboard.get(&mime).is_some();
                let q = Self::query(cx);
                let grid = cx.grid();
                if !q.symbols.is_empty()
                    || !q.net_lines.is_empty()
                    || !q.net_labels.is_empty()
                    || !q.polygons.is_empty()
                    || !q.texts.is_empty()
                    || !q.images.is_empty()
                {
                    f.cut = true;
                    f.copy = true;
                    f.remove = true;
                    f.rotate = true;
                    f.mirror = true;
                }
                if !q.symbols.is_empty() {
                    f.reset_texts = true;
                }
                if let Some(s) = cx.sch() {
                    let off = |p: Point| !p.is_on_grid(grid);
                    f.snap_to_grid = q.symbols.iter().any(|id| off(s.symbols()[id].position()))
                        || q.net_points
                            .iter()
                            .any(|(seg, j)| off(s.net_segments()[seg].junctions()[j].position()))
                        || q.net_labels
                            .iter()
                            .any(|(seg, l)| off(s.net_segments()[seg].labels()[l].position()))
                        || q.polygons
                            .iter()
                            .any(|id| !s.polygons()[id].path().is_on_grid(grid))
                        || q.texts.iter().any(|id| off(s.texts()[id].position()))
                        || q.images.iter().any(|id| off(s.images()[id].position()));
                }
                if !q.symbols.is_empty()
                    || !q.net_labels.is_empty()
                    || !q.polygons.is_empty()
                    || !q.texts.is_empty()
                {
                    f.properties = true;
                }
                cx.out.view.info_box = info_text(cx, &q);
            }
            SubState::Selecting => {}
        }
        if self.sub != SubState::Selecting {
            cx.out.view.features = f;
        }
    }
}

/// Builds the info box text of the selection (upstream
/// `processSelection()`, without MPN and cross-probing).
fn info_text(cx: &Cx<'_, '_>, q: &SelectionQuery) -> String {
    if q.count() == 0 {
        return String::new();
    }
    let p = cx.project();
    let Some(s) = cx.sch() else {
        return String::new();
    };
    let ctx = p.view();
    let mut key_values: Vec<(String, String)> = Vec::new();
    let mut net: Option<Option<librepcb_core::project::NetSignalId>> = None;
    let mut multiple_nets = false;
    let mut add_net = |n: Option<librepcb_core::project::NetSignalId>| {
        if net.is_some_and(|x| x != n) {
            multiple_nets = true;
        } else {
            net = Some(n);
        }
    };
    for (seg, _) in q.net_labels.iter().chain(&q.net_lines) {
        add_net(Some(s.net_segments()[seg].net()));
    }
    let pin_view = |(symbol, pin): &(SymbolId, librepcb_core::types::Uuid)| {
        s.symbols()
            .get(symbol)
            .and_then(|sym| sym.pin(ctx, *pin).ok().flatten())
    };
    for pin in &q.pins {
        if let Some(view) = pin_view(pin) {
            add_net(view.net());
        }
    }
    if q.symbols.len() == 1
        && let Some(symbol) = q.symbols.iter().next().and_then(|id| s.symbols().get(id))
        && let Ok(resolved) = symbol.resolve(ctx)
        && !resolved.lib_component.schematic_only()
    {
        key_values.push((
            tr!("SchematicEditorState_Select", "Name"),
            resolved.component.name().to_string(),
        ));
        key_values.push((
            tr!("SchematicEditorState_Select", "Value"),
            resolved
                .component
                .value()
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" "),
        ));
    }
    if let (Some(n), false) = (net, multiple_nets) {
        let name = n
            .and_then(|id| p.circuit().net_signal(id))
            .map_or_else(|| "✖".to_owned(), |x| x.name().to_string());
        key_values.push((tr!("SchematicEditorState_Select", "Net"), name));
        if let Some(signal) = n.and_then(|id| p.circuit().net_signal(id))
            && p.circuit().net_classes().len() > 1
            && let Some(class) = p.circuit().net_class(signal.net_class())
        {
            key_values.push((
                tr!("SchematicEditorState_Select", "Class"),
                class.name().to_string(),
            ));
        }
    }
    if q.pins.len() == 1
        && let Some(view) = q.pins.iter().next().and_then(pin_view)
    {
        key_values.push((
            tr!("SchematicEditorState_Select", "Signal"),
            view.lib_signal().name().to_string(),
        ));
        key_values.push((
            tr!("SchematicEditorState_Select", "Pin"),
            view.name().to_string(),
        ));
    }
    key_values.retain(|(_, v)| !v.is_empty());
    let max_len = key_values
        .iter()
        .map(|(k, _)| k.chars().count())
        .max()
        .unwrap_or(0);
    key_values
        .iter()
        .map(|(k, v)| {
            if k.is_empty() {
                v.clone()
            } else {
                format!("{k}: {}{v}", " ".repeat(max_len - k.chars().count()))
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

impl State for SelectState {
    fn entry(&mut self, cx: &mut Cx<'_, '_>) -> bool {
        cx.out.tool = super::SchematicTool::Select;
        self.update_features(cx);
        true
    }

    fn exit(&mut self, cx: &mut Cx<'_, '_>) -> bool {
        if matches!(self.sub, SubState::Pasting | SubState::Moving)
            && cx.ctx.editor.undo_stack().is_group_active()
            && let Err(e) = cx.ctx.editor.abort_group()
        {
            cx.error(e);
            return false;
        }
        self.drag = None;
        self.sub = SubState::Idle;
        // Avoid propagating the selection to other tools.
        cx.out.selection.clear();
        cx.out.view.rubber_band = None;
        cx.out.view.info_box.clear();
        cx.out.view.features = Features::default();
        true
    }

    fn select_all(&mut self, cx: &mut Cx<'_, '_>) -> bool {
        if self.sub != SubState::Idle {
            return false;
        }
        if let Some(s) = cx.sch() {
            cx.out.selection = all_items(s);
        }
        true
    }

    fn cut(&mut self, cx: &mut Cx<'_, '_>) -> bool {
        self.sub == SubState::Idle && self.copy_selected(cx) && self.remove_selected(cx)
    }

    fn copy(&mut self, cx: &mut Cx<'_, '_>) -> bool {
        self.sub == SubState::Idle && self.copy_selected(cx)
    }

    fn paste(&mut self, cx: &mut Cx<'_, '_>) -> bool {
        self.sub == SubState::Idle && self.paste_from_clipboard(cx)
    }

    fn move_by(&mut self, cx: &mut Cx<'_, '_>, delta: Point) -> bool {
        if self.sub != SubState::Idle || self.drag.is_some() {
            return false;
        }
        self.one_shot(cx, |d| d.set_current_position(delta, None))
    }

    fn rotate(&mut self, cx: &mut Cx<'_, '_>, angle: Angle) -> bool {
        self.rotate_selected(cx, angle)
    }

    fn mirror(&mut self, cx: &mut Cx<'_, '_>, orientation: Orientation) -> bool {
        self.mirror_selected(cx, orientation)
    }

    fn snap_to_grid(&mut self, cx: &mut Cx<'_, '_>) -> bool {
        let grid = cx.grid();
        if let Some(drag) = &mut self.drag {
            drag.snap_to_grid(grid);
            self.apply_drag_preview(cx);
            return true;
        }
        self.one_shot(cx, |d| d.snap_to_grid(grid))
    }

    fn reset_all_texts(&mut self, cx: &mut Cx<'_, '_>) -> bool {
        if self.sub != SubState::Idle {
            return false;
        }
        self.one_shot(cx, |d| d.reset_all_texts())
    }

    fn remove(&mut self, cx: &mut Cx<'_, '_>) -> bool {
        if self.sub != SubState::Idle {
            return false;
        }
        self.remove_selected(cx);
        true
    }

    fn edit_properties(&mut self, cx: &mut Cx<'_, '_>) -> bool {
        if self.sub != SubState::Idle {
            return false;
        }
        let q = Self::query(cx);
        let item = q
            .symbols
            .iter()
            .map(|id| SchematicItem::Symbol(*id))
            .chain(
                q.net_labels
                    .iter()
                    .map(|(s, l)| SchematicItem::NetLabel(*s, *l)),
            )
            .chain(q.polygons.iter().map(|id| SchematicItem::Polygon(*id)))
            .chain(q.texts.iter().map(|id| SchematicItem::Text(*id)))
            .next();
        item.is_some_and(|item| Self::open_properties(cx, item))
    }

    fn abort(&mut self, cx: &mut Cx<'_, '_>) -> bool {
        match self.sub {
            SubState::Idle => {
                cx.out.selection.clear();
                true
            }
            SubState::Pasting => {
                if let Err(e) = cx.ctx.editor.abort_group() {
                    cx.error(e);
                }
                self.drag = None;
                cx.out.selection.clear();
                self.sub = SubState::Idle;
                true
            }
            _ => false,
        }
    }

    fn pointer_moved(&mut self, cx: &mut Cx<'_, '_>, e: PointerEvent) -> bool {
        match self.sub {
            SubState::Selecting => {
                cx.out.view.rubber_band = Some((self.start_pos, e.pos));
                let items = cx.ctx.view.items_in_rect(self.start_pos, e.pos);
                let mut selection: std::collections::BTreeSet<SchematicItem> =
                    items.into_iter().collect();
                // Junctions are selected on the model.
                if let Some(s) = cx.sch() {
                    let (x0, x1) = (self.start_pos.x.min(e.pos.x), self.start_pos.x.max(e.pos.x));
                    let (y0, y1) = (self.start_pos.y.min(e.pos.y), self.start_pos.y.max(e.pos.y));
                    for (seg_id, seg) in s.net_segments() {
                        for j in seg.junctions().values() {
                            let p = j.position();
                            if p.x >= x0 && p.x <= x1 && p.y >= y0 && p.y <= y1 {
                                selection.insert(SchematicItem::NetPoint(*seg_id, j.uuid()));
                            }
                        }
                    }
                    selection.retain(|i| item_exists(i, s));
                }
                cx.out.selection = selection;
                true
            }
            SubState::Moving | SubState::Pasting => {
                if let Some(drag) = &mut self.drag {
                    let old = drag.delta();
                    drag.set_current_position(e.pos, None);
                    if drag.delta() != old {
                        self.apply_drag_preview(cx);
                    }
                }
                true
            }
            SubState::Idle => {
                let items = find_items_at(cx, e.pos, FindFlags::ALL.near(), &[]);
                cx.out.hovered = items.first().copied();
                false
            }
        }
    }

    fn left_pressed(&mut self, cx: &mut Cx<'_, '_>, e: PointerEvent) -> bool {
        match self.sub {
            SubState::Idle => {
                let items = find_items_at(cx, e.pos, FindFlags::ALL.near(), &[]);
                if items.is_empty() {
                    // No items under the cursor: start a selection rectangle.
                    cx.out.selection.clear();
                    self.start_pos = e.pos;
                    self.sub = SubState::Selecting;
                    return true;
                }
                let selected = items
                    .iter()
                    .rev()
                    .find(|i| cx.out.selection.contains(*i))
                    .copied();
                if e.modifiers.control {
                    // Toggle the selection when CTRL is pressed.
                    let item = selected.unwrap_or(items[0]);
                    if !cx.out.selection.remove(&item) {
                        cx.out.selection.insert(item);
                    }
                } else if e.modifiers.shift {
                    // Cycle the selection when holding shift.
                    let index = selected
                        .and_then(|s| items.iter().position(|i| *i == s))
                        .map_or(0, |i| (i + 1) % items.len());
                    cx.out.selection.clear();
                    cx.out.selection.insert(items[index]);
                } else if selected.is_none() {
                    // Only select the topmost item when clicking an
                    // unselected item without CTRL.
                    cx.out.selection.clear();
                    cx.out.selection.insert(items[0]);
                }
                self.start_moving(cx, e.pos)
            }
            SubState::Pasting => {
                self.finish_drag(cx, e.pos);
                false
            }
            _ => false,
        }
    }

    fn left_released(&mut self, cx: &mut Cx<'_, '_>, e: PointerEvent) -> bool {
        match self.sub {
            SubState::Selecting => {
                cx.out.view.rubber_band = None;
                self.sub = SubState::Idle;
                true
            }
            SubState::Moving => {
                self.finish_drag(cx, e.pos);
                false
            }
            _ => false,
        }
    }

    fn left_double_clicked(&mut self, cx: &mut Cx<'_, '_>, e: PointerEvent) -> bool {
        // With SHIFT or CTRL, the user is modifying the selection.
        if e.modifiers.shift || e.modifiers.control {
            return self.left_pressed(cx, e);
        }
        if self.sub != SubState::Idle {
            return false;
        }
        let items = find_items_at(cx, e.pos, FindFlags::ALL.near(), &[]);
        for item in items {
            if cx.out.selection.contains(&item) && Self::open_properties(cx, item) {
                return true;
            }
        }
        false
    }

    fn right_released(&mut self, cx: &mut Cx<'_, '_>, e: PointerEvent) -> bool {
        if self.drag.is_some() {
            return self.rotate_selected(cx, Angle::DEG90);
        }
        if self.sub != SubState::Idle {
            return false;
        }
        let items = find_items_at(cx, e.pos, FindFlags::ALL.near(), &[]);
        let Some(first) = items.first().copied() else {
            return false;
        };
        let selected = items
            .iter()
            .rev()
            .find(|i| cx.out.selection.contains(*i))
            .copied();
        let item = match selected {
            Some(item) => item,
            None => {
                cx.out.selection.clear();
                cx.out.selection.insert(first);
                first
            }
        };
        if matches!(
            item,
            SchematicItem::Symbol(_)
                | SchematicItem::NetLabel(..)
                | SchematicItem::Polygon(_)
                | SchematicItem::Text(_)
        ) {
            cx.out
                .requests
                .push(SchematicRequest::ContextMenu { item, pos: e.pos });
            return true;
        }
        false
    }

    fn update(&mut self, cx: &mut Cx<'_, '_>) {
        cx.out.view.cursor = match self.sub {
            SubState::Moving | SubState::Pasting => Some(CursorShape::ClosedHand),
            _ => None,
        };
        self.update_features(cx);
    }
}
