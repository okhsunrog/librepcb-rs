//! Port of libs/librepcb/core/project/projectattributelookup.{h,cpp}.
//!
//! Determines the value of an attribute (built-in like `{{NAME}}` or
//! user-defined) of an object within a project, for the
//! [attribute substitutor](crate::attribute::substitute). Each object kind
//! has a fixed lookup chain, e.g. a symbol asks its part, itself, its
//! component, its device, its schematic, the assembly variant and finally
//! the project; the first object knowing the key wins (even if its value is
//! empty).
//!
//! Differences to upstream: upstream stores a closure over `QPointer`s (so
//! the lookup silently returns nothing once an object is deleted); here the
//! lookup borrows the project and the objects, so it cannot outlive them.
//! The objects are passed as references (not identifiers) because the part
//! of a lookup may be a temporary value (see the BOM generator).

use chrono::Local;

use super::Project;
use super::board::{Board, BoardDevice};
use super::circuit::{AssemblyVariant, ComponentInstance};
use super::schematic::{Schematic, SchematicSymbol};
use crate::attribute::{AttributeList, substitute};
use crate::library::LibraryBaseElement;
use crate::library::dev::Part;

/// One object of a lookup chain.
#[derive(Debug, Clone, Copy)]
enum Source<'a> {
    Part(&'a Part),
    Symbol(&'a SchematicSymbol),
    Component(&'a ComponentInstance),
    Device(&'a BoardDevice),
    Schematic(&'a Schematic),
    Board(&'a Board),
    AssemblyVariant(&'a AssemblyVariant),
    Project,
}

/// Attribute lookup of an object within a [`Project`] (upstream
/// `ProjectAttributeLookup`).
///
/// ```ignore
/// let lookup = ProjectAttributeLookup::for_board(&project, board, None);
/// let text = lookup.substitute("{{PROJECT}} - {{BOARD}}");
/// ```
#[derive(Debug, Clone)]
pub struct ProjectAttributeLookup<'a> {
    project: &'a Project,
    sources: Vec<Source<'a>>,
}

impl<'a> ProjectAttributeLookup<'a> {
    /// Lookup of the project itself: assembly variant, project.
    pub fn for_project(project: &'a Project, av: Option<&'a AssemblyVariant>) -> Self {
        let mut sources = Vec::new();
        sources.extend(av.map(Source::AssemblyVariant));
        sources.push(Source::Project);
        Self { project, sources }
    }

    /// Lookup of a component instance: part, component, device, project.
    pub fn for_component(
        project: &'a Project,
        component: &'a ComponentInstance,
        device: Option<&'a BoardDevice>,
        part: Option<&'a Part>,
    ) -> Self {
        let mut sources = Vec::new();
        sources.extend(part.map(Source::Part));
        sources.push(Source::Component(component));
        sources.extend(device.map(Source::Device));
        sources.push(Source::Project);
        Self { project, sources }
    }

    /// Lookup of a schematic page: schematic, assembly variant, project.
    pub fn for_schematic(
        project: &'a Project,
        schematic: &'a Schematic,
        av: Option<&'a AssemblyVariant>,
    ) -> Self {
        let mut sources = vec![Source::Schematic(schematic)];
        sources.extend(av.map(Source::AssemblyVariant));
        sources.push(Source::Project);
        Self { project, sources }
    }

    /// Lookup of a board: board, assembly variant, project.
    pub fn for_board(
        project: &'a Project,
        board: &'a Board,
        av: Option<&'a AssemblyVariant>,
    ) -> Self {
        let mut sources = vec![Source::Board(board)];
        sources.extend(av.map(Source::AssemblyVariant));
        sources.push(Source::Project);
        Self { project, sources }
    }

    /// Lookup of a symbol in `schematic`: part, symbol, component, device,
    /// schematic, assembly variant, project.
    pub fn for_symbol(
        project: &'a Project,
        schematic: &'a Schematic,
        symbol: &'a SchematicSymbol,
        device: Option<&'a BoardDevice>,
        part: Option<&'a Part>,
        av: Option<&'a AssemblyVariant>,
    ) -> Self {
        let mut sources = Vec::new();
        sources.extend(part.map(Source::Part));
        sources.push(Source::Symbol(symbol));
        sources.extend(
            project
                .circuit
                .component_instance(symbol.component())
                .map(Source::Component),
        );
        sources.extend(device.map(Source::Device));
        sources.push(Source::Schematic(schematic));
        sources.extend(av.map(Source::AssemblyVariant));
        sources.push(Source::Project);
        Self { project, sources }
    }

    /// Lookup of a device on `board`: part, device, component, board,
    /// project.
    pub fn for_device(
        project: &'a Project,
        board: &'a Board,
        device: &'a BoardDevice,
        part: Option<&'a Part>,
    ) -> Self {
        let mut sources = Vec::new();
        sources.extend(part.map(Source::Part));
        sources.push(Source::Device(device));
        sources.extend(
            project
                .circuit
                .component_instance(device.component())
                .map(Source::Component),
        );
        sources.push(Source::Board(board));
        sources.push(Source::Project);
        Self { project, sources }
    }

    /// Returns the value of the attribute `key` (upstream `operator()`), or
    /// `None` if no object of the chain knows the key. A known key may
    /// still have an empty value.
    pub fn value(&self, key: &str) -> Option<String> {
        self.sources.iter().find_map(|s| self.query(*s, key))
    }

    /// Substitutes all variables in `text` by the values of this lookup
    /// (upstream `AttributeSubstitutor::substitute(text, lookup)`).
    pub fn substitute(&self, text: &str) -> String {
        substitute(text, |key| self.value(key), None)
    }

    /// Like [`substitute()`](Self::substitute), applying `filter` to each
    /// substituted value (e.g. to clean file names).
    pub fn substitute_filtered(
        &self,
        text: &str,
        filter: &mut dyn FnMut(&str) -> String,
    ) -> String {
        substitute(text, |key| self.value(key), Some(filter))
    }

    fn query(&self, source: Source<'a>, key: &str) -> Option<String> {
        let p = self.project;
        let locales = &p.settings.locale_order;
        match source {
            Source::Project => query_project(p, key),
            Source::AssemblyVariant(av) => match key {
                "VARIANT" => Some(av.name().to_string()),
                "VARIANT_INDEX" => Some(
                    p.circuit
                        .assembly_variants()
                        .index_of_uuid(&av.uuid())
                        .map(|i| i.to_string())
                        .unwrap_or_else(|| "-1".to_owned()),
                ),
                _ => None,
            },
            Source::Component(cmp) => attribute(cmp.attributes(), key).or_else(|| match key {
                "NAME" => Some(cmp.name().to_string()),
                "VALUE" => Some(cmp.value().to_owned()),
                "COMPONENT" => Some(
                    p.library
                        .component(&cmp.lib_component())
                        .map(|c| c.metadata().names().value(locales).to_string())
                        .unwrap_or_default(),
                ),
                _ => None,
            }),
            Source::Schematic(schematic) => match key {
                "SHEET" => Some(schematic.name().to_string()),
                "PAGE" => Some(
                    p.schematic_index(schematic.id())
                        .map_or(0, |i| i + 1)
                        .to_string(),
                ),
                _ => None,
            },
            Source::Board(board) => match key {
                "BOARD" => Some(board.name().to_string()),
                "BOARD_DIRNAME" => Some(board.directory_name().to_owned()),
                "BOARD_INDEX" => Some(
                    p.board_index(board.id())
                        .map(|i| i.to_string())
                        .unwrap_or_else(|| "-1".to_owned()),
                ),
                _ => None,
            },
            Source::Symbol(symbol) => match key {
                "NAME" => Some(symbol.name(p.view()).unwrap_or_default()),
                _ => None,
            },
            Source::Device(device) => attribute(device.attributes(), key).or_else(|| {
                let lib_device = p.library.device(&device.lib_device());
                let package = lib_device.and_then(|d| p.library.package(&d.package_uuid()));
                match key {
                    "DEVICE" => Some(
                        lib_device
                            .map(|d| d.metadata().names().value(locales).to_string())
                            .unwrap_or_default(),
                    ),
                    "PACKAGE" => Some(
                        package
                            .map(|p| p.metadata().names().value(locales).to_string())
                            .unwrap_or_default(),
                    ),
                    "FOOTPRINT" => Some(
                        package
                            .and_then(|p| p.footprints().by_uuid(&device.lib_footprint()))
                            .map(|f| f.names().value(locales).to_string())
                            .unwrap_or_default(),
                    ),
                    _ => None,
                }
            }),
            Source::Part(part) => attribute(part.attributes(), key).or_else(|| match key {
                "MPN" => Some(part.mpn().to_string()),
                "MANUFACTURER" => Some(part.manufacturer().to_string()),
                _ => None,
            }),
        }
    }
}

/// Returns the displayed value of the attribute `key` (upstream
/// `getValueTr(true)`).
fn attribute(attributes: &AttributeList, key: &str) -> Option<String> {
    attributes.by_name(key, true).map(|a| a.value_tr(true))
}

fn query_project(p: &Project, key: &str) -> Option<String> {
    if let Some(value) = attribute(&p.metadata.attributes, key) {
        return Some(value);
    }
    let file_path = p.file_path();
    Some(match key {
        "PROJECT" => p.metadata.name.to_string(),
        "PROJECT_DIRPATH" => p.path().map(|fp| fp.to_native()).unwrap_or_default(),
        "PROJECT_BASENAME" => match &file_path {
            Some(fp) => fp.basename().to_owned(),
            None => p
                .file_name
                .split_once('.')
                .map_or(p.file_name.as_str(), |(b, _)| b)
                .to_owned(),
        },
        "PROJECT_FILENAME" => p.file_name.clone(),
        "PROJECT_FILEPATH" => file_path.map(|fp| fp.to_native()).unwrap_or_default(),
        // Upstream converts the date/time to local time when loading.
        "CREATED_DATE" => p
            .metadata
            .created
            .with_timezone(&Local)
            .format("%Y-%m-%d")
            .to_string(),
        "CREATED_TIME" => p
            .metadata
            .created
            .with_timezone(&Local)
            .format("%H:%M:%S")
            .to_string(),
        "DATE" => p
            .date_time
            .with_timezone(&Local)
            .format("%Y-%m-%d")
            .to_string(),
        "TIME" => p
            .date_time
            .with_timezone(&Local)
            .format("%H:%M:%S")
            .to_string(),
        "AUTHOR" => p.metadata.author.clone(),
        "VERSION" => p.metadata.version.to_string(),
        "PAGES" => p.schematics.len().to_string(),
        // Do not translate this, must be the same for every user!
        "PAGE_X_OF_Y" => "Page {{PAGE}} of {{PAGES}}".to_owned(),
        _ => return None,
    })
}
