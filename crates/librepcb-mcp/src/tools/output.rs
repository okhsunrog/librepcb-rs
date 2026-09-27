//! Checks and outputs: `erc_run`, `export_fabrication`, `export_bom`,
//! `export_pick_place`, `export_netlist`, `jobs_list`, `render`.
//!
//! Output files default to `<project>/output/<version>/...` like the
//! default output jobs of upstream; tools return the written paths.
//! (The output job runner is not ported yet, so there is no `jobs_run`.)

use librepcb_core::export::{BomCsvWriter, PickPlaceSides, Timestamp};
use librepcb_core::fileio::FilePath;
use librepcb_core::job::OutputJobKind;
use librepcb_core::project::board::{
    BoardFabricationOutputSettings, ExportInfo, export_d356_netlist, export_fabrication_data,
    export_pick_place_csv,
};
use librepcb_core::project::erc::run_erc;
use librepcb_core::project::{AssemblyVariantId, BomGenerator, Project};
use librepcb_core::rule_check::Severity;
use librepcb_scene::{BoardSide, RenderOptions, RenderSize};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::error::{ToolError, ToolResult};
use crate::outcome::ToolOutput;
use crate::resolve;
use crate::session::{Session, absolute_path};
use crate::views;

/// Arguments of `erc_run`.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct ErcArgs {
    /// Also list approved (ignored) messages (default false).
    #[serde(default)]
    pub include_approved: bool,
}

/// Arguments of `export_fabrication`.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct ExportFabricationArgs {
    /// Board name, index or UUID (default: first board).
    #[serde(default)]
    pub board: Option<String>,
    /// Output directory (default: the board's fabrication output settings,
    /// normally `<project>/output/<version>/gerber/`).
    #[serde(default)]
    pub output_dir: Option<String>,
}

/// Arguments of `export_bom`.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct ExportBomArgs {
    /// Board name, index or UUID: BOM of the devices on this board;
    /// default: generic BOM of all components.
    #[serde(default)]
    pub board: Option<String>,
    /// Assembly variant name or UUID (default: the first one).
    #[serde(default)]
    pub assembly_variant: Option<String>,
    /// Output CSV file (default `<project>/output/<version>/<project>_BOM-<variant>.csv`).
    #[serde(default)]
    pub output: Option<String>,
}

/// Board side argument.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SideArg {
    /// Top side.
    #[default]
    Top,
    /// Bottom side.
    Bottom,
    /// Both sides.
    Both,
}

/// Arguments of `export_pick_place`.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct ExportPickPlaceArgs {
    /// Board name, index or UUID (default: first board).
    #[serde(default)]
    pub board: Option<String>,
    /// Assembly variant name or UUID (default: the first one).
    #[serde(default)]
    pub assembly_variant: Option<String>,
    /// Board side(s) (default both).
    #[serde(default)]
    pub side: Option<SideArg>,
    /// Output CSV file (default `<project>/output/<version>/assembly/<project>_PnP-<side>.csv`).
    #[serde(default)]
    pub output: Option<String>,
}

/// Arguments of `export_netlist`.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct ExportNetlistArgs {
    /// Board name, index or UUID (default: first board).
    #[serde(default)]
    pub board: Option<String>,
    /// Output file (default `<project>/output/<version>/<project>_netlist.d356`).
    #[serde(default)]
    pub output: Option<String>,
}

/// What to render.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RenderTarget {
    /// A schematic page.
    #[default]
    Schematic,
    /// A board side.
    Board,
}

/// Arguments of `render`.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct RenderArgs {
    /// "schematic" (default) or "board".
    #[serde(default)]
    pub target: RenderTarget,
    /// Schematic page name, index or UUID (default: first page).
    #[serde(default)]
    pub schematic: Option<String>,
    /// Board name, index or UUID (default: first board).
    #[serde(default)]
    pub board: Option<String>,
    /// Board side seen from ("top" default, or "bottom").
    #[serde(default)]
    pub side: Option<SideArg>,
    /// Image width in pixels (default 1600, max 4096).
    #[serde(default)]
    pub width: Option<u32>,
    /// Image height in pixels (default 1200, max 4096).
    #[serde(default)]
    pub height: Option<u32>,
}

fn severity_str(s: Severity) -> &'static str {
    match s {
        Severity::Hint => "hint",
        Severity::Warning => "warning",
        Severity::Error => "error",
    }
}

/// `erc_run`.
pub fn erc_run(session: &Session, args: ErcArgs) -> ToolResult<ToolOutput> {
    let p = &session.project()?.project;
    let messages = run_erc(p);
    let mut list = Vec::new();
    let (mut errors, mut warnings, mut hints, mut approved) = (0, 0, 0, 0);
    for m in &messages {
        let is_approved = p.erc_approvals().contains(m.approval());
        let msg = m.message();
        if is_approved {
            approved += 1;
        } else {
            match msg.severity() {
                Severity::Error => errors += 1,
                Severity::Warning => warnings += 1,
                Severity::Hint => hints += 1,
            }
        }
        if is_approved && !args.include_approved {
            continue;
        }
        list.push(json!({
            "severity": severity_str(msg.severity()),
            "message": msg.message(),
            "description": msg.description(),
            "kind": format!("{:?}", m.kind()),
            "schematic": m.schematic().and_then(|id| p.schematic(id)).map(|s| s.name().as_str()),
            "approved": is_approved,
            "locations": msg.locations().iter().map(views::path).collect::<Vec<_>>(),
        }));
    }
    list.sort_by(|a, b| {
        let rank = |v: &Value| match v["severity"].as_str() {
            Some("error") => 0,
            Some("warning") => 1,
            _ => 2,
        };
        rank(a)
            .cmp(&rank(b))
            .then_with(|| a["message"].as_str().cmp(&b["message"].as_str()))
    });
    let lines: Vec<String> = list
        .iter()
        .filter(|m| m["approved"] == json!(false))
        .map(|m| {
            format!(
                "[{}] {}",
                m["severity"].as_str().unwrap_or_default().to_uppercase(),
                m["message"].as_str().unwrap_or_default()
            )
        })
        .collect();
    let summary = format!(
        "ERC ran: {errors} error(s), {warnings} warning(s), {hints} hint(s), {approved} \
         approved.{}{}",
        if lines.is_empty() { "" } else { "\n" },
        lines.join("\n")
    );
    ToolOutput::new(
        summary,
        json!({
            "ran": true,
            "errors": errors,
            "warnings": warnings,
            "hints": hints,
            "approved": approved,
            "messages": list,
        }),
    )
}

/// Default output directory `<project>/output/<version>`.
fn output_dir(p: &Project) -> ToolResult<FilePath> {
    let dir = p
        .path()
        .ok_or_else(|| ToolError::internal("the project has no directory on disk"))?;
    let version = FilePath::clean_file_name(
        p.metadata().version.as_str(),
        librepcb_core::fileio::CleanFileNameOptions::new(
            true,
            librepcb_core::fileio::FileNameCase::Keep,
        ),
        120,
    );
    Ok(dir.path_to(&format!("output/{version}")))
}

fn project_file_base(p: &Project) -> String {
    FilePath::clean_file_name(
        p.metadata().name.as_str(),
        librepcb_core::fileio::CleanFileNameOptions::new(
            true,
            librepcb_core::fileio::FileNameCase::Keep,
        ),
        120,
    )
}

fn assembly_variant(p: &Project, key: Option<&str>) -> ToolResult<(AssemblyVariantId, String)> {
    let variants = p.circuit().assembly_variants();
    let av = match key.map(str::trim).filter(|k| !k.is_empty()) {
        None => variants.iter().next(),
        Some(k) => variants.iter().find(|av| {
            av.name().as_str() == k || resolve::parse_uuid(k).is_some_and(|u| u == av.uuid())
        }),
    };
    av.map(|av| (AssemblyVariantId(av.uuid()), av.name().as_str().to_owned()))
        .ok_or_else(|| {
            ToolError::not_found(format!(
                "There is no assembly variant \"{}\".",
                key.unwrap_or_default()
            ))
        })
}

fn output_file(explicit: Option<&str>, default: FilePath) -> ToolResult<FilePath> {
    match explicit {
        Some(path) => absolute_path(path),
        None => Ok(default),
    }
}

/// `export_fabrication`.
pub fn export_fabrication(
    session: &mut Session,
    args: ExportFabricationArgs,
) -> ToolResult<ToolOutput> {
    let project = &mut session.project_mut()?.project;
    let (_, board, b) = resolve::board(project, args.board.as_deref())?;
    let settings: Option<BoardFabricationOutputSettings> = match &args.output_dir {
        Some(dir) => {
            let dir = absolute_path(dir)?;
            let mut s = b.settings().fabrication_output_settings.clone();
            s.output_base_path = format!("{}/{{{{PROJECT}}}}", dir.as_str());
            Some(s)
        }
        None => None,
    };
    let info = ExportInfo::now(env!("CARGO_PKG_VERSION"));
    let files = export_fabrication_data(project, board, settings.as_ref(), &info)?;
    let files: Vec<String> = files.iter().map(FilePath::to_native).collect();
    ToolOutput::new(
        format!(
            "Exported {} Gerber/Excellon file(s):\n{}",
            files.len(),
            files.join("\n")
        ),
        json!({ "files": files }),
    )
}

/// `export_bom`.
pub fn export_bom(session: &Session, args: ExportBomArgs) -> ToolResult<ToolOutput> {
    let p = &session.project()?.project;
    let board = match &args.board {
        Some(b) => Some(resolve::board(p, Some(b))?.2),
        None => None,
    };
    let (variant, variant_name) = assembly_variant(p, args.assembly_variant.as_deref())?;
    let mut generator = BomGenerator::new(p);
    generator.set_additional_attributes(p.settings().custom_bom_attributes.clone());
    let bom = generator.generate(board, variant);
    let csv = BomCsvWriter::new(&bom).generate_csv()?;
    let default = output_dir(p)?.path_to(&format!(
        "{}_BOM-{}.csv",
        project_file_base(p),
        variant_name
    ));
    let file = output_file(args.output.as_deref(), default)?;
    csv.save_to_file(&file)?;
    let content = csv.to_csv_string()?;
    let rows = content.lines().count().saturating_sub(1);
    ToolOutput::new(
        format!("Wrote BOM with {rows} line(s) to {}.", file.to_native()),
        json!({
            "file": file.to_native(),
            "assembly_variant": variant_name,
            "csv": if content.len() <= 65536 { Some(content) } else { None },
        }),
    )
}

/// `export_pick_place`.
pub fn export_pick_place(session: &Session, args: ExportPickPlaceArgs) -> ToolResult<ToolOutput> {
    let p = &session.project()?.project;
    let (_, board, _) = resolve::board(p, args.board.as_deref())?;
    let (variant, variant_name) = assembly_variant(p, args.assembly_variant.as_deref())?;
    let (sides, side_name) = match args.side.unwrap_or(SideArg::Both) {
        SideArg::Top => (PickPlaceSides::Top, "top"),
        SideArg::Bottom => (PickPlaceSides::Bottom, "bottom"),
        SideArg::Both => (PickPlaceSides::Both, "both"),
    };
    let default = output_dir(p)?.path_to(&format!(
        "assembly/{}_PnP-{}.csv",
        project_file_base(p),
        side_name
    ));
    let file = output_file(args.output.as_deref(), default)?;
    export_pick_place_csv(
        p,
        board,
        variant,
        sides,
        &file,
        env!("CARGO_PKG_VERSION"),
        Timestamp::now(),
    )?;
    let content = std::fs::read_to_string(file.as_path()).unwrap_or_default();
    ToolOutput::new(
        format!("Wrote pick&place data to {}.", file.to_native()),
        json!({
            "file": file.to_native(),
            "assembly_variant": variant_name,
            "side": side_name,
            "csv": if content.len() <= 65536 { Some(content) } else { None },
        }),
    )
}

/// `export_netlist`.
pub fn export_netlist(session: &Session, args: ExportNetlistArgs) -> ToolResult<ToolOutput> {
    let p = &session.project()?.project;
    let (_, board, _) = resolve::board(p, args.board.as_deref())?;
    let default = output_dir(p)?.path_to(&format!("{}_netlist.d356", project_file_base(p)));
    let file = output_file(args.output.as_deref(), default)?;
    let info = ExportInfo::now(env!("CARGO_PKG_VERSION"));
    export_d356_netlist(p, board, &file, &info)?;
    ToolOutput::new(
        format!("Wrote IPC-D-356A netlist to {}.", file.to_native()),
        json!({ "file": file.to_native() }),
    )
}

/// `jobs_list`.
pub fn jobs_list(session: &Session) -> ToolResult<ToolOutput> {
    let p = &session.project()?.project;
    let jobs: Vec<Value> = p
        .output_jobs()
        .iter()
        .map(|j| {
            json!({
                "uuid": j.uuid(),
                "name": j.name().as_str(),
                "type": job_type(j.kind()),
            })
        })
        .collect();
    ToolOutput::new(
        format!(
            "{} output job(s). Running output jobs is not supported yet; use the export_* \
             tools.",
            jobs.len()
        ),
        json!({ "jobs": jobs }),
    )
}

fn job_type(kind: &OutputJobKind) -> String {
    let debug = format!("{kind:?}");
    debug
        .split(|c: char| !c.is_alphanumeric())
        .next()
        .unwrap_or_default()
        .to_owned()
}

/// `render`.
pub fn render(session: &Session, args: RenderArgs) -> ToolResult<ToolOutput> {
    let p = &session.project()?.project;
    let width = args.width.unwrap_or(1600).clamp(64, 4096);
    let height = args.height.unwrap_or(1200).clamp(64, 4096);
    let options = RenderOptions {
        size: RenderSize::Fit { width, height },
        ..RenderOptions::default()
    };
    let (png, what) = match args.target {
        RenderTarget::Schematic => {
            let (index, id, s) = resolve::schematic(p, args.schematic.as_deref())?;
            (
                librepcb_scene::render_schematic_png(p, id, &options)?,
                format!("schematic page {index} \"{}\"", s.name().as_str()),
            )
        }
        RenderTarget::Board => {
            let (index, id, b) = resolve::board(p, args.board.as_deref())?;
            let side = match args.side.unwrap_or(SideArg::Top) {
                SideArg::Bottom => BoardSide::Bottom,
                _ => BoardSide::Top,
            };
            (
                librepcb_scene::render_board_png(p, id, side, &options)?,
                format!(
                    "board {index} \"{}\" ({} side)",
                    b.name().as_str(),
                    if side == BoardSide::Top {
                        "top"
                    } else {
                        "bottom"
                    }
                ),
            )
        }
    };
    Ok(ToolOutput::new(
        format!("Rendered {what} ({width}x{height} px)."),
        json!({ "width": width, "height": height, "bytes": png.len(), "mime_type": "image/png" }),
    )?
    .image(png))
}
