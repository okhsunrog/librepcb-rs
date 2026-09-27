//! Port of libs/librepcb/core/project/board/items/bi_netsegment.{h,cpp}
//! (with `bi_netpoint`, `bi_netline`, `bi_via` and the standalone
//! `bi_pad`, which become the geometry types [`Junction`], [`Trace`],
//! [`Via`] and [`BoardPadData`] stored in the segment).
//!
//! Differences to upstream: the elements are plain data keyed by UUID; the
//! anchors of traces are [`TraceAnchor`]s resolved through the board when
//! needed. The registration of traces at their anchors
//! (`registerNetLine()`) becomes the connectivity queries below
//! ([`BoardNetSegment::traces_at()`], [`BoardNetSegment::junction_layer()`])
//! and the validation in the project mutations. The cohesion check
//! (`areAllNetPointsConnectedTogether()`) uses a union-find over the
//! anchors instead of a recursive search.

use std::collections::{BTreeMap, BTreeSet};

use petgraph::unionfind::UnionFind;

use super::BoardPadData;
use crate::geometry::{Junction, Trace, TraceAnchor, Via};
use crate::project::id::{ComponentInstanceId, NetSegmentId, NetSignalId};
use crate::serialization::{self, DeserializeObject, List, SExpression, SerializeObject};
use crate::types::{Layer, Uuid};

/// Elements of a board net segment, e.g. to add to or update in a segment
/// (upstream `BI_NetSegment::addElements()` arguments).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BoardSegmentElements {
    /// Standalone pads.
    pub pads: Vec<BoardPadData>,
    /// Vias.
    pub vias: Vec<Via>,
    /// Junctions.
    pub junctions: Vec<Junction>,
    /// Traces.
    pub traces: Vec<Trace>,
}

impl BoardSegmentElements {
    /// Whether no element is listed.
    pub fn is_empty(&self) -> bool {
        self.pads.is_empty()
            && self.vias.is_empty()
            && self.junctions.is_empty()
            && self.traces.is_empty()
    }

    /// Returns the UUIDs of the elements.
    pub fn uuids(&self) -> super::BoardSegmentItems {
        super::BoardSegmentItems {
            pads: self.pads.iter().map(BoardPadData::uuid).collect(),
            vias: self.vias.iter().map(Via::uuid).collect(),
            junctions: self.junctions.iter().map(Junction::uuid).collect(),
            traces: self.traces.iter().map(Trace::uuid).collect(),
        }
    }
}

/// A net segment of a board: connected copper (standalone pads, vias,
/// junctions and traces) of one net (or no net).
///
/// Serde: an object with the fields below (the element maps keyed by UUID).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BoardNetSegment {
    uuid: Uuid,
    net: Option<NetSignalId>,
    #[serde(with = "super::keyed_map")]
    pads: BTreeMap<Uuid, BoardPadData>,
    #[serde(with = "super::keyed_map")]
    vias: BTreeMap<Uuid, Via>,
    #[serde(with = "super::keyed_map")]
    junctions: BTreeMap<Uuid, Junction>,
    #[serde(with = "super::keyed_map")]
    traces: BTreeMap<Uuid, Trace>,
}

impl BoardNetSegment {
    /// Creates an empty net segment.
    pub fn new(uuid: Uuid, net: Option<NetSignalId>) -> Self {
        Self {
            uuid,
            net,
            pads: BTreeMap::new(),
            vias: BTreeMap::new(),
            junctions: BTreeMap::new(),
            traces: BTreeMap::new(),
        }
    }

    /// Creates a net segment with elements (later elements replace earlier
    /// ones with the same UUID).
    pub fn with_elements(
        uuid: Uuid,
        net: Option<NetSignalId>,
        pads: impl IntoIterator<Item = BoardPadData>,
        vias: impl IntoIterator<Item = Via>,
        junctions: impl IntoIterator<Item = Junction>,
        traces: impl IntoIterator<Item = Trace>,
    ) -> Self {
        Self {
            uuid,
            net,
            pads: pads.into_iter().map(|p| (p.uuid(), p)).collect(),
            vias: vias.into_iter().map(|v| (v.uuid(), v)).collect(),
            junctions: junctions.into_iter().map(|j| (j.uuid(), j)).collect(),
            traces: traces.into_iter().map(|t| (t.uuid(), t)).collect(),
        }
    }

    /// Returns the UUID.
    pub fn uuid(&self) -> Uuid {
        self.uuid
    }

    /// Returns the typed identifier.
    pub fn id(&self) -> NetSegmentId {
        NetSegmentId(self.uuid)
    }

    /// Returns the net signal (`None` = no net).
    pub fn net(&self) -> Option<NetSignalId> {
        self.net
    }

    /// Sets the net signal, returns whether it was modified.
    pub fn set_net(&mut self, net: Option<NetSignalId>) -> bool {
        if net == self.net {
            return false;
        }
        self.net = net;
        true
    }

    /// Returns the standalone pads.
    pub fn pads(&self) -> &BTreeMap<Uuid, BoardPadData> {
        &self.pads
    }

    /// Returns the vias.
    pub fn vias(&self) -> &BTreeMap<Uuid, Via> {
        &self.vias
    }

    /// Returns the junctions (upstream net points).
    pub fn junctions(&self) -> &BTreeMap<Uuid, Junction> {
        &self.junctions
    }

    /// Returns the traces (upstream net lines).
    pub fn traces(&self) -> &BTreeMap<Uuid, Trace> {
        &self.traces
    }

    /// Adds or replaces a standalone pad, returns the replaced one.
    pub fn insert_pad(&mut self, pad: BoardPadData) -> Option<BoardPadData> {
        self.pads.insert(pad.uuid(), pad)
    }

    /// Adds or replaces a via, returns the replaced one.
    pub fn insert_via(&mut self, via: Via) -> Option<Via> {
        self.vias.insert(via.uuid(), via)
    }

    /// Adds or replaces a junction, returns the replaced one.
    pub fn insert_junction(&mut self, junction: Junction) -> Option<Junction> {
        self.junctions.insert(junction.uuid(), junction)
    }

    /// Adds or replaces a trace, returns the replaced one.
    pub fn insert_trace(&mut self, trace: Trace) -> Option<Trace> {
        self.traces.insert(trace.uuid(), trace)
    }

    /// Removes a standalone pad.
    pub fn remove_pad(&mut self, uuid: &Uuid) -> Option<BoardPadData> {
        self.pads.remove(uuid)
    }

    /// Removes a via.
    pub fn remove_via(&mut self, uuid: &Uuid) -> Option<Via> {
        self.vias.remove(uuid)
    }

    /// Removes a junction.
    pub fn remove_junction(&mut self, uuid: &Uuid) -> Option<Junction> {
        self.junctions.remove(uuid)
    }

    /// Removes a trace.
    pub fn remove_trace(&mut self, uuid: &Uuid) -> Option<Trace> {
        self.traces.remove(uuid)
    }

    /// Whether the segment contains no elements at all.
    pub fn is_empty(&self) -> bool {
        self.pads.is_empty() && !self.is_used()
    }

    /// Whether the segment contains vias, junctions or traces (upstream
    /// `isUsed()`; standalone pads alone don't count).
    pub fn is_used(&self) -> bool {
        !self.vias.is_empty() || !self.junctions.is_empty() || !self.traces.is_empty()
    }

    /// Whether the anchor is an element of this segment (footprint pads
    /// are never elements of a segment).
    pub fn contains_anchor(&self, anchor: &TraceAnchor) -> bool {
        match anchor {
            TraceAnchor::Junction(uuid) => self.junctions.contains_key(uuid),
            TraceAnchor::Via(uuid) => self.vias.contains_key(uuid),
            TraceAnchor::Pad(uuid) => self.pads.contains_key(uuid),
            TraceAnchor::FootprintPad { .. } => false,
        }
    }

    /// Returns the traces connected to an anchor (upstream
    /// `BI_NetLineAnchor::getNetLines()`, restricted to this segment).
    pub fn traces_at(&self, anchor: TraceAnchor) -> impl Iterator<Item = &Trace> {
        self.traces
            .values()
            .filter(move |t| (t.p1() == anchor) || (t.p2() == anchor))
    }

    /// Returns the layer of the traces connected to a junction (upstream
    /// `BI_NetPoint::getLayerOfTraces()`), `None` if no trace is
    /// connected. All traces of a junction are on the same layer.
    pub fn junction_layer(&self, junction: &Uuid) -> Option<Layer> {
        self.traces_at(TraceAnchor::Junction(*junction))
            .next()
            .map(Trace::layer)
    }

    /// Returns the footprint pads the traces are connected to, as
    /// (component instance, footprint pad) pairs.
    pub fn footprint_pads(&self) -> BTreeSet<(ComponentInstanceId, Uuid)> {
        self.traces
            .values()
            .flat_map(|t| [t.p1(), t.p2()])
            .filter_map(|a| match a {
                TraceAnchor::FootprintPad { device, pad } => {
                    Some((ComponentInstanceId(device), pad))
                }
                _ => None,
            })
            .collect()
    }

    /// Whether all elements are connected together by traces (upstream
    /// `areAllNetPointsConnectedTogether()`); an empty segment is
    /// cohesive.
    pub fn is_cohesive(&self) -> bool {
        let mut anchors: BTreeMap<TraceAnchor, usize> = BTreeMap::new();
        let mut index = |anchor: TraceAnchor| {
            let next = anchors.len();
            *anchors.entry(anchor).or_insert(next)
        };
        let mut elements: Vec<usize> = Vec::new();
        elements.extend(self.pads.keys().map(|u| index(TraceAnchor::Pad(*u))));
        elements.extend(self.vias.keys().map(|u| index(TraceAnchor::Via(*u))));
        elements.extend(
            self.junctions
                .keys()
                .map(|u| index(TraceAnchor::Junction(*u))),
        );
        let edges: Vec<(usize, usize)> = self
            .traces
            .values()
            .map(|t| (index(t.p1()), index(t.p2())))
            .collect();
        elements.extend(edges.iter().map(|(a, _)| *a));
        let mut union_find = UnionFind::<usize>::new(anchors.len());
        for (a, b) in edges {
            union_find.union(a, b);
        }
        match elements.first() {
            Some(first) => {
                let root = union_find.find(*first);
                elements.iter().all(|e| union_find.find(*e) == root)
            }
            None => true,
        }
    }
}

impl SerializeObject for BoardNetSegment {
    fn serialize(&self, root: &mut List) {
        root.append_value(&self.uuid);
        root.ensure_line_break();
        root.append_child("net", &self.net);
        root.ensure_line_break();
        for pad in self.pads.values() {
            root.ensure_line_break();
            pad.serialize(root.append_list("pad"));
        }
        root.ensure_line_break();
        for via in self.vias.values() {
            root.ensure_line_break();
            via.serialize(root.append_list("via"));
        }
        root.ensure_line_break();
        for junction in self.junctions.values() {
            root.ensure_line_break();
            junction.serialize(root.append_list("junction"));
        }
        root.ensure_line_break();
        for trace in self.traces.values() {
            root.ensure_line_break();
            trace.serialize(root.append_list("trace"));
        }
        root.ensure_line_break();
    }
}

impl DeserializeObject for BoardNetSegment {
    /// Reads the segment and its elements (duplicate element UUIDs: the
    /// last one wins; the loader rejects them before, like upstream).
    fn deserialize(node: &SExpression) -> serialization::Result<Self> {
        Ok(Self {
            uuid: node.child_value("@0")?,
            net: node.child_value("net/@0")?,
            pads: node
                .children_named("pad")
                .map(|n| BoardPadData::deserialize(n).map(|p| (p.uuid(), p)))
                .collect::<serialization::Result<_>>()?,
            vias: node
                .children_named("via")
                .map(|n| Via::deserialize(n).map(|v| (v.uuid(), v)))
                .collect::<serialization::Result<_>>()?,
            junctions: node
                .children_named("junction")
                .map(|n| Junction::deserialize(n).map(|j| (j.uuid(), j)))
                .collect::<serialization::Result<_>>()?,
            traces: node
                .children_named("trace")
                .map(|n| Trace::deserialize(n).map(|t| (t.uuid(), t)))
                .collect::<serialization::Result<_>>()?,
        })
    }
}
