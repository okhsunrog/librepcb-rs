//! Schematic mutations: pages (upstream `Project::addSchematic()`,
//! `removeSchematic()`, `Schematic::setName()`) and, in
//! [`SchematicMutation`], the items (upstream `Schematic::addSymbol()`,
//! `SI_NetSegment::addNetPointsAndNetLines()`, the item setters, ...).
//!
//! TODO(wave3b/schematic): add the item mutations (`AddSymbol`,
//! `RemoveSymbol`, `SetSymbolPlacement`, `AddNetSegment`,
//! `AddNetSegmentElements`, `RemoveNetSegmentElements`, `SetNetSegmentNet`,
//! `AddNetLabel`, bus segments, polygons, texts, images, ...) with their
//! validation (pin/net consistency, cohesion, one segment per pin) and
//! reverse index updates (`p.refs.insert()`/`remove()`).

use super::Mutation;
use crate::fileio::FileSystem;
use crate::project::Project;
use crate::project::change::Change;
use crate::project::error::{EntityKind, Error, Result};
use crate::project::id::SchematicId;
use crate::project::schematic::{Schematic, SchematicProperties};

/// A change of the items of a schematic, see
/// [`Mutation::Schematic`](super::Mutation::Schematic).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub enum SchematicMutation {
    /// Placeholder until the items are ported (never applied successfully).
    #[doc(hidden)]
    Unimplemented(SchematicId),
}

pub(super) fn apply(_p: &mut Project, m: SchematicMutation) -> Result<Mutation> {
    // TODO(wave3b/schematic)
    match m {
        SchematicMutation::Unimplemented(id) => Err(Error::NotFound {
            kind: EntityKind::Schematic,
            uuid: id.0,
        }),
    }
}

pub(super) fn add_schematic(
    p: &mut Project,
    schematic: Schematic,
    index: Option<usize>,
) -> Result<Mutation> {
    let id = schematic.id();
    if p.schematic(id).is_some() {
        return Err(Error::DuplicateUuid {
            kind: EntityKind::Schematic,
            uuid: id.0,
        });
    }
    check_schematic_name(p, schematic.name(), None)?;
    if p.schematics
        .iter()
        .any(|s| s.directory_name() == schematic.directory_name())
    {
        return Err(Error::DuplicateDirectoryName {
            kind: EntityKind::Schematic,
            name: schematic.directory_name().to_owned(),
        });
    }
    // TODO(wave3b/schematic): validate the items against circuit and
    // library like `Schematic::addToProject()` (all items `addToSchematic()`).
    for r in schematic.references(id) {
        p.refs.insert(r);
    }
    let index = index.unwrap_or(p.schematics.len()).min(p.schematics.len());
    p.schematics.insert(index, schematic);
    p.record(Change::SchematicAdded(id));
    p.record(Change::ProjectMetadata);
    Ok(Mutation::RemoveSchematic(id))
}

pub(super) fn remove_schematic(p: &mut Project, id: SchematicId) -> Result<Mutation> {
    let index = p.schematic_index(id).ok_or(Error::NotFound {
        kind: EntityKind::Schematic,
        uuid: id.0,
    })?;
    let schematic = &p.schematics[index];
    if !schematic.is_empty() {
        return Err(Error::SchematicNotEmpty(schematic.name().to_string()));
    }
    // upstream: moves the schematic directory into a temporary file system
    p.directory
        .remove_dir_recursively(&format!("schematics/{}", schematic.directory_name()))?;
    let schematic = p.schematics.remove(index);
    p.refs.remove_schematic(id);
    p.record(Change::SchematicRemoved(id));
    p.record(Change::ProjectMetadata);
    Ok(Mutation::AddSchematic {
        schematic,
        index: Some(index),
    })
}

pub(super) fn update_schematic(
    p: &mut Project,
    properties: SchematicProperties,
) -> Result<Mutation> {
    let id = properties.id;
    let index = p.schematic_index(id).ok_or(Error::NotFound {
        kind: EntityKind::Schematic,
        uuid: id.0,
    })?;
    check_schematic_name(p, &properties.name, Some(id))?;
    let old = p.schematics[index].set_properties(properties);
    p.record(Change::SchematicChanged(id));
    p.record(Change::ProjectMetadata);
    Ok(Mutation::UpdateSchematic(old))
}

fn check_schematic_name(p: &Project, name: &str, except: Option<SchematicId>) -> Result<()> {
    if p.schematic_by_name(name)
        .is_some_and(|s| Some(s.id()) != except)
    {
        return Err(Error::DuplicateName {
            kind: EntityKind::Schematic,
            name: name.to_owned(),
        });
    }
    Ok(())
}
