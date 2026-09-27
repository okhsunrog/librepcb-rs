//! Port of libs/librepcb/editor/project/schematic/schematicselectionquery.{h,cpp}:
//! the selected model objects of a schematic page, resolved from the FSM's
//! selection set (only items which still exist).

use std::collections::{BTreeMap, BTreeSet};

use librepcb_core::geometry::NetLineAnchor;
use librepcb_core::project::schematic::Schematic;
use librepcb_core::project::{NetSegmentId, SymbolId};
use librepcb_core::types::Uuid;

use super::SchematicItem;

/// The selected items of a page by kind (upstream
/// `SchematicSelectionQuery`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SelectionQuery {
    pub symbols: BTreeSet<SymbolId>,
    pub pins: BTreeSet<(SymbolId, Uuid)>,
    pub net_points: BTreeSet<(NetSegmentId, Uuid)>,
    pub net_lines: BTreeSet<(NetSegmentId, Uuid)>,
    pub net_labels: BTreeSet<(NetSegmentId, Uuid)>,
    pub polygons: BTreeSet<Uuid>,
    pub texts: BTreeSet<Uuid>,
    pub images: BTreeSet<Uuid>,
}

/// The selected junctions, lines and labels of one net segment (upstream
/// `SchematicSelectionQuery::NetSegmentItems`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct NetSegmentItems {
    pub net_points: BTreeSet<Uuid>,
    pub net_lines: BTreeSet<Uuid>,
    pub net_labels: BTreeSet<Uuid>,
}

impl SelectionQuery {
    /// Collects the selected items which exist on the page (upstream
    /// `addSelected*()`).
    pub fn new(selection: &BTreeSet<SchematicItem>, s: &Schematic) -> Self {
        let mut q = Self::default();
        let seg_has = |seg: &NetSegmentId,
                       f: &dyn Fn(
            &librepcb_core::project::schematic::SchematicNetSegment,
        ) -> bool| { s.net_segments().get(seg).is_some_and(f) };
        for item in selection {
            match *item {
                SchematicItem::Symbol(id) if s.symbols().contains_key(&id) => {
                    q.symbols.insert(id);
                }
                SchematicItem::SymbolPin(id, pin) if s.symbols().contains_key(&id) => {
                    q.pins.insert((id, pin));
                }
                SchematicItem::NetPoint(seg, j)
                    if seg_has(&seg, &|x| x.junctions().contains_key(&j)) =>
                {
                    q.net_points.insert((seg, j));
                }
                SchematicItem::NetLine(seg, l)
                    if seg_has(&seg, &|x| x.lines().contains_key(&l)) =>
                {
                    q.net_lines.insert((seg, l));
                }
                SchematicItem::NetLabel(seg, l)
                    if seg_has(&seg, &|x| x.labels().contains_key(&l)) =>
                {
                    q.net_labels.insert((seg, l));
                }
                SchematicItem::Polygon(id) if s.polygons().contains_key(&id) => {
                    q.polygons.insert(id);
                }
                SchematicItem::Text(id) if s.texts().contains_key(&id) => {
                    q.texts.insert(id);
                }
                SchematicItem::Image(id) if s.images().contains_key(&id) => {
                    q.images.insert(id);
                }
                _ => {}
            }
        }
        q
    }

    /// Adds the junctions of the selected lines (upstream
    /// `addNetPointsOfNetLines()`); with `only_if_all_lines_selected`, only
    /// junctions whose lines are all selected.
    pub fn add_net_points_of_net_lines(&mut self, s: &Schematic, only_if_all_lines_selected: bool) {
        let mut add = BTreeSet::new();
        for (seg_id, line) in &self.net_lines {
            let Some(seg) = s.net_segments().get(seg_id) else {
                continue;
            };
            let Some(line) = seg.lines().get(line) else {
                continue;
            };
            for anchor in [line.p1(), line.p2()] {
                if let NetLineAnchor::Junction(j) = anchor {
                    let all = seg
                        .lines_at(anchor)
                        .all(|l| self.net_lines.contains(&(*seg_id, l.uuid())));
                    if !only_if_all_lines_selected || all {
                        add.insert((*seg_id, j));
                    }
                }
            }
        }
        self.net_points.extend(add);
    }

    /// The selected junctions, lines and labels per net segment (upstream
    /// `getNetSegmentItems()`).
    pub fn net_segment_items(&self) -> BTreeMap<NetSegmentId, NetSegmentItems> {
        let mut result: BTreeMap<NetSegmentId, NetSegmentItems> = BTreeMap::new();
        for (seg, j) in &self.net_points {
            result.entry(*seg).or_default().net_points.insert(*j);
        }
        for (seg, l) in &self.net_lines {
            result.entry(*seg).or_default().net_lines.insert(*l);
        }
        for (seg, l) in &self.net_labels {
            result.entry(*seg).or_default().net_labels.insert(*l);
        }
        result
    }

    /// Number of selected items (upstream `getResultCount()`).
    pub fn count(&self) -> usize {
        self.symbols.len()
            + self.pins.len()
            + self.net_points.len()
            + self.net_lines.len()
            + self.net_labels.len()
            + self.polygons.len()
            + self.texts.len()
            + self.images.len()
    }
}

/// Returns whether an item still exists on the page.
pub(crate) fn item_exists(item: &SchematicItem, s: &Schematic) -> bool {
    let seg = |id: &NetSegmentId| s.net_segments().get(id);
    match item {
        SchematicItem::Symbol(id) => s.symbols().contains_key(id),
        SchematicItem::SymbolPin(id, _) => s.symbols().contains_key(id),
        SchematicItem::NetPoint(id, j) => seg(id).is_some_and(|x| x.junctions().contains_key(j)),
        SchematicItem::NetLine(id, l) => seg(id).is_some_and(|x| x.lines().contains_key(l)),
        SchematicItem::NetLabel(id, l) => seg(id).is_some_and(|x| x.labels().contains_key(l)),
        SchematicItem::BusJunction(id, j) => s
            .bus_segments()
            .get(id)
            .is_some_and(|x| x.junctions().contains_key(j)),
        SchematicItem::BusLine(id, l) => s
            .bus_segments()
            .get(id)
            .is_some_and(|x| x.lines().contains_key(l)),
        SchematicItem::BusLabel(id, l) => s
            .bus_segments()
            .get(id)
            .is_some_and(|x| x.labels().contains_key(l)),
        SchematicItem::Polygon(id) => s.polygons().contains_key(id),
        SchematicItem::Text(id) => s.texts().contains_key(id),
        SchematicItem::Image(id) => s.images().contains_key(id),
    }
}

/// All items of a page which can be selected (upstream `selectAll()`).
pub(crate) fn all_items(s: &Schematic) -> BTreeSet<SchematicItem> {
    let mut items = BTreeSet::new();
    for id in s.symbols().keys() {
        items.insert(SchematicItem::Symbol(*id));
    }
    for (id, seg) in s.net_segments() {
        for j in seg.junctions().keys() {
            items.insert(SchematicItem::NetPoint(*id, *j));
        }
        for l in seg.lines().keys() {
            items.insert(SchematicItem::NetLine(*id, *l));
        }
        for l in seg.labels().keys() {
            items.insert(SchematicItem::NetLabel(*id, *l));
        }
    }
    for (id, seg) in s.bus_segments() {
        for j in seg.junctions().keys() {
            items.insert(SchematicItem::BusJunction(*id, *j));
        }
        for l in seg.lines().keys() {
            items.insert(SchematicItem::BusLine(*id, *l));
        }
        for l in seg.labels().keys() {
            items.insert(SchematicItem::BusLabel(*id, *l));
        }
    }
    for id in s.polygons().keys() {
        items.insert(SchematicItem::Polygon(*id));
    }
    for id in s.texts().keys() {
        items.insert(SchematicItem::Text(*id));
    }
    for id in s.images().keys() {
        items.insert(SchematicItem::Image(*id));
    }
    items
}
