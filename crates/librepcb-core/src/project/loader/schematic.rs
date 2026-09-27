//! Schematic part of the project loader (upstream
//! `ProjectLoader::loadSchematics()`, `loadSchematic()`,
//! `loadSchematicSymbol()`, `loadSchematicBusSegment()`,
//! `loadSchematicNetSegment()`, `loadSchematicUserSettings()`).
//!
//! TODO(wave3b/schematic): load the items (symbols with texts, bus
//! segments, net segments with the anchor resolution and its errors,
//! polygons, texts, images) and drop the verbatim `raw` copy.

use super::{parse_file, split_index_path};
use crate::project::Project;
use crate::project::error::Result;
use crate::project::schematic::Schematic;

pub(super) fn load_schematics(p: &mut Project) -> Result<()> {
    let index = parse_file(p, "schematics/schematics.lp")?;
    for node in index.children_named("schematic") {
        let relative: String = node.child_value("@0")?;
        load_schematic(p, &relative)?;
    }
    log::debug!("Successfully loaded schematics.");
    Ok(())
}

fn load_schematic(p: &mut Project, relative_file_path: &str) -> Result<()> {
    let (dir, directory_name) = split_index_path(relative_file_path);
    let root = parse_file(p, relative_file_path)?;
    let mut schematic = Schematic::new(
        root.child_value("@0")?,
        root.child_value("name/@0")?,
        directory_name,
    );
    schematic.set_grid_interval(root.child_value("grid/interval/@0")?);
    schematic.set_grid_unit(root.child_value("grid/unit/@0")?);
    schematic.raw = Some(root);
    // TODO(wave3b/schematic): items.
    p.add_schematic(schematic, None)?;
    load_schematic_user_settings(p, &dir);
    Ok(())
}

fn load_schematic_user_settings(p: &Project, dir: &str) {
    // upstream: parses `settings.user.lp` (currently without content) and
    // uses defaults on error.
    if let Err(e) = parse_file(p, &format!("{dir}/settings.user.lp")) {
        log::error!("Could not load schematic user settings, defaults will be used instead: {e}");
    }
}
