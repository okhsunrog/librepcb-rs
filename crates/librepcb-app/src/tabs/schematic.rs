//! The schematic tab.
//!
//! Port of libs/librepcb/editor/project/schematic/schematictab.{h,cpp}
//! (viewer part): the schematic page rendered by `librepcb-scene`'s
//! [`SchematicScene`], navigation, grid, pin number visibility, and a
//! read-only select tool (click selects the object under the cursor, the
//! status bar describes the hovered object). The editor state machine
//! (`SchematicEditorFsm`) follows in M3.

use std::rc::Rc;

use librepcb_app_ui as ui;
use librepcb_canvas::kurbo::Point;
use librepcb_canvas::peniko::Color;
use librepcb_canvas::{Grid, PointerAction, PointerButton, PointerKind};
use librepcb_core::project::SchematicId;
use librepcb_core::types::{GridStyle, Length, LengthUnit, PositiveLength};
use librepcb_i18n::tr;
use librepcb_scene::{ColorScheme, SchematicObject, SchematicScene};

use super::{TabId, TabUpdate, feature, project_index};
use crate::canvas_view::{CanvasView, DEFAULT_SCHEMATIC_RECT};
use crate::helpers::{
    color_to_ui, grid_style_from_ui, grid_style_to_ui, length_from_ui, length_to_ui, unit_from_ui,
    unit_to_ui,
};
use crate::project::AppProject;

/// Grid color of the default schematic color scheme (upstream
/// `schematicLibrePcbLight()`, secondary color of the background role).
const GRID_COLOR: Color = Color::from_rgb8(0xa0, 0xa0, 0xa4);

/// Hit test tolerance in logical pixels (upstream
/// `calcPosWithTolerance()`).
const HIT_TOLERANCE_PX: f64 = 5.0;

/// A schematic page tab.
pub struct SchematicTab {
    id: TabId,
    project: Rc<AppProject>,
    schematic: SchematicId,
    scheme: ColorScheme,
    scene: Option<SchematicScene>,
    /// Project revision the scene was built from.
    scene_revision: u64,
    canvas: CanvasView,
    title: String,
    grid_style: GridStyle,
    grid_interval: PositiveLength,
    unit: LengthUnit,
    show_pin_numbers: bool,
    selected: Option<SchematicObject>,
    frame: i32,
    scene_image_pos: slint::LogicalPosition,
}

impl SchematicTab {
    /// Creates the tab of a schematic page.
    pub fn new(project: Rc<AppProject>, schematic: SchematicId, grid_style: GridStyle) -> Self {
        let scheme = ColorScheme::SCHEMATIC_LIGHT;
        let (scene, revision, title, grid_interval, unit) = {
            let p = project.shared().lock();
            let proj = p.project();
            let scene = SchematicScene::build(proj, schematic, &scheme)
                .map_err(|e| log::error!("Failed to build the schematic scene: {e}"))
                .ok();
            let props = proj.schematic(schematic).map(|s| s.properties());
            (
                scene,
                proj.revision(),
                props
                    .as_ref()
                    .map(|p| p.name.to_string())
                    .unwrap_or_default(),
                props.as_ref().map_or(
                    PositiveLength::new(Length::new(2_540_000)).expect("positive"),
                    |p| p.grid_interval,
                ),
                props.map_or(LengthUnit::Millimeters, |p| p.grid_unit),
            )
        };
        let background = scene
            .as_ref()
            .map_or(Color::WHITE, SchematicScene::background);
        let content = scene.as_ref().and_then(SchematicScene::content_bounds);
        let mut tab = Self {
            id: TabId::new(),
            project,
            schematic,
            scheme,
            scene,
            scene_revision: revision,
            canvas: CanvasView::new(background, DEFAULT_SCHEMATIC_RECT, content),
            title,
            grid_style,
            grid_interval,
            unit,
            show_pin_numbers: true,
            selected: None,
            frame: 0,
            scene_image_pos: Default::default(),
        };
        tab.apply_grid();
        tab
    }

    /// The unique tab identifier.
    pub fn id(&self) -> TabId {
        self.id
    }

    /// The project.
    pub fn project(&self) -> &Rc<AppProject> {
        &self.project
    }

    /// The schematic shown.
    pub fn schematic(&self) -> SchematicId {
        self.schematic
    }

    /// The graphics view.
    pub fn canvas_mut(&mut self) -> &mut CanvasView {
        &mut self.canvas
    }

    /// The scene (if it could be built).
    pub fn scene(&self) -> Option<&SchematicScene> {
        self.scene.as_ref()
    }

    /// Sets the grid style (a workspace setting shared by all tabs).
    pub fn set_grid_style(&mut self, style: GridStyle) -> TabUpdate {
        if style == self.grid_style {
            return TabUpdate::default();
        }
        self.grid_style = style;
        self.apply_grid();
        TabUpdate::repaint()
    }

    fn apply_grid(&mut self) {
        let interval = self.grid_interval.get().to_mm();
        let grid = match self.grid_style {
            GridStyle::None => None,
            GridStyle::Dots => Some(librepcb_canvas::GridStyle::Dots),
            GridStyle::Lines => Some(librepcb_canvas::GridStyle::Lines),
        }
        .map(|style| Grid {
            interval,
            style,
            color: GRID_COLOR,
        });
        self.canvas.set_grid(grid);
    }

    fn apply_pin_numbers(&mut self) {
        let visible = self.show_pin_numbers;
        if let Some(scene) = &mut self.scene
            && let Some(layer) = SchematicScene::layer_id("schematic_pin_numbers")
        {
            scene.scene_mut().set_layer_visible(layer, visible);
        }
    }

    /// Rebuilds the scene if the project changed since it was built.
    pub fn rebuild_if_modified(&mut self) -> TabUpdate {
        let p = self.project.shared().lock();
        let proj = p.project();
        if proj.revision() == self.scene_revision {
            return TabUpdate::default();
        }
        match SchematicScene::build(proj, self.schematic, &self.scheme) {
            Ok(scene) => {
                self.scene = Some(scene);
                self.scene_revision = proj.revision();
                if let Some(s) = proj.schematic(self.schematic) {
                    self.title = s.properties().name.to_string();
                }
            }
            Err(e) => log::error!("Failed to rebuild the schematic scene: {e}"),
        }
        drop(p);
        self.selected = None;
        self.apply_pin_numbers();
        TabUpdate {
            repaint: true,
            data_changed: true,
            ..TabUpdate::default()
        }
    }

    /// The `TabData`.
    pub fn ui_data(&self) -> ui::TabData {
        let p = self.project.shared().lock();
        let undo = p.editor.undo_stack();
        let writable = self.project.is_writable();
        let features = ui::TabFeatures {
            save: feature(writable),
            undo: feature(undo.can_undo()),
            redo: feature(undo.can_redo()),
            grid: feature(writable),
            zoom: feature(true),
            export_graphics: ui::FeatureState::NotSupported,
            select: ui::FeatureState::NotSupported,
            find: ui::FeatureState::NotSupported,
            ..Default::default()
        };
        ui::TabData {
            r#type: ui::TabType::Schematic,
            title: self.title.as_str().into(),
            features,
            read_only: !writable,
            unsaved_changes: p.has_unsaved_changes(),
            undo_text: undo.undo_text().unwrap_or_default().into(),
            redo_text: undo.redo_text().unwrap_or_default().into(),
            ..Default::default()
        }
    }

    /// The `SchematicTabData`.
    pub fn derived_ui_data(&self, projects: &[Rc<AppProject>]) -> ui::SchematicTabData {
        let background = self
            .scene
            .as_ref()
            .map_or(Color::WHITE, SchematicScene::background);
        let [r, g, b, _] = background.to_rgba8().to_u8_array();
        let lightness = (f32::from(r.max(g).max(b)) + f32::from(r.min(g).min(b))) / 510.0;
        let foreground = if lightness >= 0.5 {
            slint::Color::from_rgb_u8(0, 0, 0)
        } else {
            slint::Color::from_rgb_u8(255, 255, 255)
        };
        let schematic_index = {
            let p = self.project.shared().lock();
            p.project()
                .schematic_index(self.schematic)
                .map_or(-1, |i| i as i32)
        };
        ui::SchematicTabData {
            project_index: project_index(projects, &self.project),
            schematic_index,
            background_color: color_to_ui(background).into(),
            foreground_color: foreground.into(),
            // Upstream: primary and secondary color of the info box role.
            overlay_color: slint::Color::from_argb_u8(0x82, 0xff, 0xff, 0xff).into(),
            overlay_text_color: slint::Color::from_rgb_u8(0, 0, 0).into(),
            grid_style: grid_style_to_ui(self.grid_style),
            grid_interval: length_to_ui(self.grid_interval.get()),
            unit: unit_to_ui(self.unit),
            show_pin_numbers: self.show_pin_numbers,
            tool: ui::EditorTool::Select,
            scene_image_pos: self.scene_image_pos,
            frame: self.frame,
            ..Default::default()
        }
    }

    /// Applies `SchematicTabData` written by the UI.
    pub fn set_derived_ui_data(&mut self, data: &ui::SchematicTabData) -> TabUpdate {
        let mut update = TabUpdate::default();
        self.scene_image_pos = data.scene_image_pos;
        let style = grid_style_from_ui(data.grid_style);
        if style != self.grid_style {
            update = self.set_grid_style(style);
        }
        if let Ok(interval) = PositiveLength::new(length_from_ui(data.grid_interval.clone()))
            && interval != self.grid_interval
        {
            // Upstream modifies the schematic's grid (a user setting
            // without undo); the viewer keeps it per tab for now.
            self.grid_interval = interval;
            self.apply_grid();
            update.repaint = true;
        }
        let unit = unit_from_ui(data.unit);
        if unit != self.unit {
            self.unit = unit;
            update.data_changed = true;
        }
        if data.show_pin_numbers != self.show_pin_numbers {
            self.show_pin_numbers = data.show_pin_numbers;
            self.apply_pin_numbers();
            update.repaint = true;
        }
        update
    }

    /// Handles a tab action.
    pub fn trigger(&mut self, action: ui::TabAction) -> TabUpdate {
        let content = self.scene.as_ref().and_then(SchematicScene::content_bounds);
        match action {
            ui::TabAction::ZoomFit => self.canvas.zoom_fit(content),
            ui::TabAction::ZoomIn => self.canvas.zoom_in(),
            ui::TabAction::ZoomOut => self.canvas.zoom_out(),
            ui::TabAction::GridIntervalIncrease => {
                self.grid_interval = PositiveLength::new(self.grid_interval.get().scaled(2.0))
                    .unwrap_or(self.grid_interval);
                self.apply_grid();
            }
            ui::TabAction::GridIntervalDecrease => {
                let half = Length::new(self.grid_interval.get().to_nm() / 2);
                if let Ok(half) = PositiveLength::new(half) {
                    self.grid_interval = half;
                }
                self.apply_grid();
            }
            ui::TabAction::Abort | ui::TabAction::ToolSelect => {
                self.select(None);
            }
            _ => return TabUpdate::default(),
        }
        TabUpdate {
            repaint: true,
            data_changed: true,
            ..TabUpdate::default()
        }
    }

    /// Renders the scene.
    pub fn render_scene(&mut self, width: f32, height: f32, scale_factor: f32) -> slint::Image {
        let Some(scene) = &self.scene else {
            return slint::Image::default();
        };
        self.canvas.render(
            scene.scene(),
            scene.content_bounds(),
            width,
            height,
            scale_factor,
        )
    }

    /// Handles a pointer event.
    pub fn pointer_event(
        &mut self,
        kind: PointerKind,
        button: PointerButton,
        pos: Point,
    ) -> TabUpdate {
        match self.canvas.pointer_event(kind, button, pos) {
            PointerAction::ViewChanged => TabUpdate::repaint(),
            PointerAction::LeftPressed(world) => {
                let object = self.object_at(world);
                self.select(object);
                TabUpdate::repaint()
            }
            PointerAction::Moved(world) => TabUpdate {
                cursor: Some((world, self.unit)),
                status: Some(
                    self.object_at(world)
                        .map(|o| self.describe(o))
                        .unwrap_or_default(),
                ),
                ..TabUpdate::default()
            },
            _ => TabUpdate::default(),
        }
    }

    fn object_at(&self, world: Point) -> Option<SchematicObject> {
        let scene = self.scene.as_ref()?;
        let tolerance = self.canvas.view().pixels_to_world(HIT_TOLERANCE_PX);
        let item = scene.scene().hit_test(world, tolerance)?;
        scene.object(item)
    }

    /// Selects all items of an object (or clears the selection).
    fn select(&mut self, object: Option<SchematicObject>) {
        self.selected = object;
        let Some(scene) = &mut self.scene else {
            return;
        };
        let items: Vec<_> = match object {
            Some(o) => scene
                .scene()
                .items()
                .map(|(id, _)| id)
                .filter(|id| scene.object(*id) == Some(o))
                .collect(),
            None => Vec::new(),
        };
        let canvas = scene.scene_mut();
        canvas.clear_selection();
        for id in items {
            canvas.set_selected(id, true);
        }
    }

    /// A short description of an object for the status bar.
    fn describe(&self, object: SchematicObject) -> String {
        match object {
            SchematicObject::Symbol(id) | SchematicObject::SymbolPin(id, _) => {
                let p = self.project.shared().lock();
                let project = p.project();
                let name = project
                    .schematic(self.schematic)
                    .and_then(|s| s.symbols().get(&id))
                    .and_then(|s| s.name(project.view()).ok())
                    .unwrap_or_default();
                if matches!(object, SchematicObject::SymbolPin(..)) {
                    tr!("SchematicTab", "Pin of {0}", name)
                } else {
                    tr!("SchematicTab", "Symbol {0}", name)
                }
            }
            SchematicObject::NetLine(..) | SchematicObject::NetJunction(..) => {
                tr!("SchematicTab", "Wire")
            }
            SchematicObject::NetLabel(..) => tr!("SchematicTab", "Net Label"),
            SchematicObject::BusLine(..)
            | SchematicObject::BusJunction(..)
            | SchematicObject::BusLabel(..) => tr!("SchematicTab", "Bus"),
            SchematicObject::Polygon(_) => tr!("SchematicTab", "Polygon"),
            SchematicObject::Text(_) => tr!("SchematicTab", "Text"),
            SchematicObject::Image(_) => tr!("SchematicTab", "Image"),
        }
    }

    /// Bumps the frame counter (the UI then requests a new image).
    pub fn bump_frame(&mut self) {
        self.frame = self.frame.wrapping_add(1);
    }
}
