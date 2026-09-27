//! Errors of the [`project`](super) module.
//!
//! Replaces the `RuntimeError`/`LogicError` exceptions of the upstream
//! project classes and of `ProjectLoader`. Upstream's lifecycle
//! `LogicError`s (object not added, added twice, wrong parent) have no
//! counterpart: an entity exists iff it is in its map. Data level rules keep
//! the upstream messages; those translated upstream go through [`tr!`].

use std::fmt;

use librepcb_i18n::tr;

use crate::fileio::FilePath;
use crate::types::{Uuid, Version};

/// Result type of the project module.
pub type Result<T, E = Error> = std::result::Result<T, E>;

fn native(path: &Option<FilePath>) -> String {
    path.as_ref().map(FilePath::to_native).unwrap_or_default()
}

/// Kind of a project entity, for error messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum EntityKind {
    /// An assembly variant.
    AssemblyVariant,
    /// A net class.
    NetClass,
    /// A net signal.
    NetSignal,
    /// A bus.
    Bus,
    /// A component instance.
    Component,
    /// A schematic page.
    Schematic,
    /// A board.
    Board,
    /// A symbol of a schematic.
    Symbol,
    /// A net segment of a schematic or board.
    NetSegment,
    /// A bus segment of a schematic.
    BusSegment,
    /// A device of a board.
    Device,
    /// A plane of a board.
    Plane,
    /// A library element of the project library.
    LibraryElement,
}

impl fmt::Display for EntityKind {
    /// The (untranslated) noun used in upstream messages, e.g. `net class`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::AssemblyVariant => "assembly variant",
            Self::NetClass => "net class",
            Self::NetSignal => "net signal",
            Self::Bus => "bus",
            Self::Component => "component",
            Self::Schematic => "schematic",
            Self::Board => "board",
            Self::Symbol => "symbol",
            Self::NetSegment => "netsegment",
            Self::BusSegment => "bus segment",
            Self::Device => "device",
            Self::Plane => "plane",
            Self::LibraryElement => "library element",
        })
    }
}

/// Formats the translated "There is already a ... with the name" message
/// (upstream uses one string per class, each in its own context).
fn duplicate_name(kind: EntityKind, name: &str) -> String {
    match kind {
        EntityKind::AssemblyVariant => tr!(
            "librepcb::Circuit",
            "There is already an assembly variant with the name \"{0}\"!",
            name
        ),
        EntityKind::NetClass => tr!(
            "librepcb::Circuit",
            "There is already a net class with the name \"{0}\"!",
            name
        ),
        EntityKind::NetSignal => tr!(
            "librepcb::Circuit",
            "There is already a net signal with the name \"{0}\"!",
            name
        ),
        EntityKind::Bus => tr!(
            "librepcb::Circuit",
            "There is already a bus with the name \"{0}\"!",
            name
        ),
        EntityKind::Component => tr!(
            "librepcb::Circuit",
            "There is already a component with the name \"{0}\"!",
            name
        ),
        EntityKind::Schematic => tr!(
            "librepcb::Project",
            "There is already a schematic with the name \"{0}\"!",
            name
        ),
        EntityKind::Board => tr!(
            "librepcb::Project",
            "There is already a board with the name \"{0}\"!",
            name
        ),
        _ => format!("There is already a {kind} with the name \"{name}\"!"),
    }
}

/// Error returned when loading, saving or modifying a project fails.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// File access failed.
    #[error(transparent)]
    FileIo(#[from] crate::fileio::Error),
    /// A file could not be parsed or deserialized.
    #[error(transparent)]
    Serialization(#[from] crate::serialization::Error),
    /// A library element of the project library could not be loaded or
    /// saved.
    #[error(transparent)]
    Library(#[from] crate::library::Error),
    /// A value is invalid.
    #[error(transparent)]
    Types(#[from] crate::types::Error),
    /// A stroke font of the project could not be loaded.
    #[error(transparent)]
    Font(#[from] crate::font::Error),

    // --- Opening ---
    /// The project file (`*.lpp`) does not exist.
    #[error("{}", tr!("librepcb::ProjectLoader", "File does not exist: '{0}'", .0.to_native()))]
    ProjectFileNotFound(FilePath),
    /// The directory contains no `.librepcb-project` file.
    #[error(
        "{}",
        tr!(
            "librepcb::ProjectLoader",
            "Directory does not contain a LibrePCB project: '{0}'",
            native(.0)
        )
    )]
    NotAProjectDirectory(Option<FilePath>),
    /// The project was written by a newer application (file format).
    #[error(
        "{}",
        tr!(
            "librepcb::ProjectLoader",
            "This project was created with a newer application version.\n\
             You need at least LibrePCB {0} to open it.\n\n{1}",
            .version.to_pretty_str(3, 10),
            .path.to_native()
        )
    )]
    NewerFileFormat {
        /// File format version of the project.
        version: Version,
        /// The project file.
        path: FilePath,
    },
    /// Upgrading the project from an older file format failed.
    #[error(transparent)]
    Migration(#[from] crate::serialization::MigrationError),
    /// The project file name does not end with `.lpp`.
    #[error("{}", tr!("librepcb::Project", "The suffix of the project file must be \"lpp\"!"))]
    InvalidProjectFileSuffix,
    /// The directory already contains a project (when creating one).
    #[error(
        "{}",
        tr!(
            "librepcb::Project",
            "The directory \"{0}\" already contains a LibrePCB project.",
            native(.0)
        )
    )]
    ProjectDirectoryNotEmpty(Option<FilePath>),
    /// The circuit file contains no assembly variant.
    #[error("Project has no assembly variants.")]
    NoAssemblyVariants,

    // --- References ---
    /// A referenced entity does not exist (upstream `LogicError` when
    /// operating on an object which is not in the circuit/project).
    #[error("There is no {kind} with the UUID \"{uuid}\".")]
    NotFound {
        /// Kind of the entity.
        kind: EntityKind,
        /// The UUID.
        uuid: Uuid,
    },
    /// A net class referenced by a net signal does not exist.
    #[error("Inexistent net class: '{0}'")]
    InexistentNetClass(Uuid),
    /// A net signal referenced by a component signal or segment does not
    /// exist.
    #[error("Inexistent net signal: '{0}'")]
    InexistentNetSignal(Uuid),
    /// A bus referenced by a bus segment does not exist.
    #[error("Inexistent bus: '{0}'")]
    InexistentBus(Uuid),
    /// A component instance references a library component which is not in
    /// the project library.
    #[error("The component '{0}' does not exist in the project's library.")]
    MissingLibraryComponent(Uuid),
    /// A component instance references a signal which the library
    /// component does not have.
    #[error("Inexistent component signal: '{0}'")]
    InexistentComponentSignal(Uuid),
    /// A component signal is listed twice in a component instance.
    #[error("The signal '{0}' is defined multiple times.")]
    DuplicateComponentSignal(Uuid),
    /// The signals of a component instance do not match its library
    /// component.
    #[error(
        "The signal count of the component instance '{component}' does not match with \
         the signal count of the component '{lib_component}'."
    )]
    SignalCountMismatch {
        /// The component instance.
        component: Uuid,
        /// The library component.
        lib_component: Uuid,
    },

    // --- Uniqueness ---
    /// An entity with the same UUID already exists.
    #[error("There is already a {kind} with the UUID \"{uuid}\"!")]
    DuplicateUuid {
        /// Kind of the entity.
        kind: EntityKind,
        /// The UUID.
        uuid: Uuid,
    },
    /// An entity with the same name already exists.
    #[error("{}", duplicate_name(*.kind, .name))]
    DuplicateName {
        /// Kind of the entity.
        kind: EntityKind,
        /// The name.
        name: String,
    },
    /// A schematic or board with the same directory name already exists.
    #[error(
        "{}",
        match .kind {
            EntityKind::Board => tr!(
                "librepcb::Project",
                "There is already a board with the directory name \"{0}\"!",
                .name
            ),
            _ => tr!(
                "librepcb::Project",
                "There is already a schematic with the directory name \"{0}\"!",
                .name
            ),
        }
    )]
    DuplicateDirectoryName {
        /// Kind of the entity (schematic or board).
        kind: EntityKind,
        /// The directory name.
        name: String,
    },
    /// A library element with the same UUID is already in the project
    /// library.
    #[error("There is already an element with the same UUID in the project's library: {0}")]
    DuplicateLibraryElement(Uuid),

    // --- Removal ---
    /// A schematic can only be removed when it is empty.
    #[error(
        "{}",
        tr!("librepcb::Project", "There are still elements in the schematic \"{0}\"!", .0)
    )]
    SchematicNotEmpty(String),
    /// The last assembly variant cannot be removed.
    #[error("The last assembly variant cannot be removed!")]
    LastAssemblyVariant,
    /// A net class still has net signals.
    #[error(
        "{}",
        tr!(
            "librepcb::NetClass",
            "The net class \"{0}\" cannot be removed because it is still in use!",
            .0
        )
    )]
    NetClassInUse(String),
    /// A net signal is still referenced (component signals, segments,
    /// planes).
    #[error(
        "{}",
        tr!(
            "librepcb::NetSignal",
            "The net signal \"{0}\" cannot be removed because it is still in use!",
            .0
        )
    )]
    NetSignalInUse(String),
    /// A bus still has bus segments.
    #[error("The bus \"{0}\" cannot be removed because it is still in use!")]
    BusInUse(String),
    /// A component instance still has symbols or devices, or its signals
    /// are still connected.
    #[error(
        "{}",
        tr!(
            "librepcb::ComponentInstance",
            "The component \"{0}\" cannot be removed because it is still in use!",
            .0
        )
    )]
    ComponentInUse(String),
    /// The net of a component signal cannot change while its pins or pads
    /// are connected.
    #[error(
        "{}",
        tr!(
            "librepcb::ComponentSignalInstance",
            "The net signal of the component signal \"{0}:{1}\" cannot be changed because it \
             is still in use!",
            .component,
            .signal
        )
    )]
    ComponentSignalInUse {
        /// Name of the component instance.
        component: String,
        /// Name of the component signal.
        signal: String,
    },
}
