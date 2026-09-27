//! Minimal port of libs/librepcb/core/project/projectattributelookup.{h,cpp}
//! for substituting `{{KEY}}` variables in schematic and board texts.
//!
//! TODO: replace by the core `ProjectAttributeLookup` once it is ported
//! (wave 3c). Differences to upstream until then: assembly variants
//! (`VARIANT`, `VARIANT_INDEX`) are not looked up, and texts of devices and
//! symbols use the first part of the component (like upstream's painters,
//! which pass no assembly variant).

use librepcb_core::attribute::{AttributeList, substitute};
use librepcb_core::library::LibraryBaseElement;
use librepcb_core::library::dev::Part;
use librepcb_core::project::board::{Board, BoardDevice};
use librepcb_core::project::circuit::ComponentInstance;
use librepcb_core::project::schematic::{Schematic, SchematicSymbol};
use librepcb_core::project::{Project, SchematicId};

fn attribute(list: &AttributeList, key: &str) -> Option<String> {
    list.by_name(key, true).map(|a| a.value_tr(true))
}

fn query_project(p: &Project, key: &str) -> Option<String> {
    let m = p.metadata();
    if let Some(v) = attribute(&m.attributes, key) {
        return Some(v);
    }
    let file_path = p.file_path();
    Some(match key {
        "PROJECT" => m.name.as_str().to_owned(),
        "PROJECT_DIRPATH" => p.path().map(|f| f.to_native()).unwrap_or_default(),
        "PROJECT_BASENAME" => file_path.map(|f| f.basename().to_owned())?,
        "PROJECT_FILENAME" => p.file_name().to_owned(),
        "PROJECT_FILEPATH" => file_path.map(|f| f.to_native())?,
        "CREATED_DATE" => m.created.format("%Y-%m-%d").to_string(),
        "CREATED_TIME" => m.created.format("%H:%M:%S").to_string(),
        "DATE" => p.date_time().format("%Y-%m-%d").to_string(),
        "TIME" => p.date_time().format("%H:%M:%S").to_string(),
        "AUTHOR" => m.author.clone(),
        "VERSION" => m.version.as_str().to_owned(),
        "PAGES" => p.schematics().len().to_string(),
        // Not translated, must be the same for every user (upstream).
        "PAGE_X_OF_Y" => "Page {{PAGE}} of {{PAGES}}".to_owned(),
        _ => return None,
    })
}

fn query_schematic(p: &Project, s: &Schematic, key: &str) -> Option<String> {
    match key {
        "SHEET" => Some(s.properties().name.as_str().to_owned()),
        "PAGE" => p
            .schematic_index(SchematicId(s.uuid()))
            .map(|i| (i + 1).to_string()),
        _ => None,
    }
}

fn query_board(p: &Project, b: &Board, key: &str) -> Option<String> {
    match key {
        "BOARD" => Some(b.properties().name.as_str().to_owned()),
        "BOARD_DIRNAME" => Some(b.directory_name().to_owned()),
        "BOARD_INDEX" => p.board_index(b.id()).map(|i| i.to_string()),
        _ => None,
    }
}

fn query_component(p: &Project, c: &ComponentInstance, key: &str) -> Option<String> {
    if let Some(v) = attribute(c.attributes(), key) {
        return Some(v);
    }
    match key {
        "NAME" => Some(c.name().as_str().to_owned()),
        "VALUE" => Some(c.value().clone()),
        "COMPONENT" => p
            .library()
            .component(&c.lib_component())
            .map(|lib| lib.metadata().name().as_str().to_owned()),
        _ => None,
    }
}

fn query_device(p: &Project, d: &BoardDevice, key: &str) -> Option<String> {
    if let Some(v) = attribute(d.attributes(), key) {
        return Some(v);
    }
    let lib = p.library();
    let device = lib.device(&d.lib_device());
    match key {
        "DEVICE" => device.map(|dev| dev.metadata().name().as_str().to_owned()),
        "PACKAGE" => device
            .and_then(|dev| lib.package(&dev.package_uuid()))
            .map(|pkg| pkg.metadata().name().as_str().to_owned()),
        "FOOTPRINT" => device
            .and_then(|dev| lib.package(&dev.package_uuid()))
            .and_then(|pkg| pkg.footprints().by_uuid(&d.lib_footprint()))
            .map(|f| f.names().default_value().as_str().to_owned()),
        _ => None,
    }
}

fn query_part(part: &Part, key: &str) -> Option<String> {
    if let Some(v) = attribute(part.attributes(), key) {
        return Some(v);
    }
    match key {
        "MPN" => Some(part.mpn().as_str().to_owned()),
        "MANUFACTURER" => Some(part.manufacturer().as_str().to_owned()),
        _ => None,
    }
}

/// The device of a component on the first board which has one (upstream
/// `ComponentInstance::getPrimaryDevice()`).
pub(crate) fn primary_device<'a>(p: &'a Project, c: &ComponentInstance) -> Option<&'a BoardDevice> {
    p.boards().iter().find_map(|b| b.device(c.id()))
}

/// Substitutes the variables of a text of a schematic symbol.
pub(crate) fn substitute_symbol_text(
    p: &Project,
    schematic: &Schematic,
    symbol: &SchematicSymbol,
    text: &str,
) -> String {
    let component = p.circuit().component_instance(symbol.component());
    let part = component.and_then(|c| c.parts(None).into_iter().next());
    let device = component.and_then(|c| primary_device(p, c));
    substitute(
        text,
        |key| {
            part.as_ref()
                .and_then(|part| query_part(part, key))
                .or_else(|| match key {
                    "NAME" => symbol.name(p.view()).ok(),
                    _ => None,
                })
                .or_else(|| component.and_then(|c| query_component(p, c, key)))
                .or_else(|| device.and_then(|d| query_device(p, d, key)))
                .or_else(|| query_schematic(p, schematic, key))
                .or_else(|| query_project(p, key))
        },
        None,
    )
}

/// Substitutes the variables of a text of a schematic page.
pub(crate) fn substitute_schematic_text(p: &Project, schematic: &Schematic, text: &str) -> String {
    substitute(
        text,
        |key| query_schematic(p, schematic, key).or_else(|| query_project(p, key)),
        None,
    )
}

/// Substitutes the variables of a stroke text of a board.
pub(crate) fn substitute_board_text(p: &Project, board: &Board, text: &str) -> String {
    substitute(
        text,
        |key| query_board(p, board, key).or_else(|| query_project(p, key)),
        None,
    )
}

/// Substitutes the variables of a stroke text of a device.
pub(crate) fn substitute_device_text(
    p: &Project,
    board: &Board,
    device: &BoardDevice,
    text: &str,
) -> String {
    let component = p.circuit().component_instance(device.component());
    let part = component.and_then(|c| c.parts(None).into_iter().next());
    substitute(
        text,
        |key| {
            part.as_ref()
                .and_then(|part| query_part(part, key))
                .or_else(|| query_device(p, device, key))
                .or_else(|| component.and_then(|c| query_component(p, c, key)))
                .or_else(|| query_board(p, board, key))
                .or_else(|| query_project(p, key))
        },
        None,
    )
}
