//! The board 2D tab.
//!
//! Port of libs/librepcb/editor/project/board/board2dtab.{h,cpp} (viewer
//! part): the board rendered by `librepcb-scene`'s [`BoardScene`], the
//! layers panel (visibility of the board layers), top/bottom view,
//! navigation, grid, and a read-only select tool. The editor state machine
//! (`BoardEditorFsm`), DRC, planes rebuild and component placement follow
//! in later milestones.

use std::rc::Rc;

use librepcb_app_ui as ui;
use librepcb_canvas::kurbo::Point;
use librepcb_canvas::peniko::Color;
use librepcb_canvas::{Grid, PointerAction, PointerButton, PointerKind};
use librepcb_core::project::BoardId;
use librepcb_core::types::{GridStyle, Layer, Length, LengthUnit, PositiveLength};
use librepcb_i18n::tr;
use librepcb_scene::{BoardObject, BoardScene, BoardSceneLayer, BoardSide, ColorScheme, SceneSync};

use super::{TabId, TabUpdate, feature, project_index};
use crate::canvas_view::{CanvasView, DEFAULT_BOARD_RECT};
use crate::helpers::{
    color_to_ui, grid_style_from_ui, grid_style_to_ui, length_from_ui, length_to_ui, unit_from_ui,
    unit_to_ui,
};
use crate::models::{UiModel, model_rc};
use crate::project::AppProject;

/// Grid color of the default board color scheme (upstream
/// `boardLibrePcbDark()`, secondary color of the background role).
const GRID_COLOR: Color = Color::from_rgb8(0xa0, 0xa0, 0xa4);

/// Hit test tolerance in logical pixels.
const HIT_TOLERANCE_PX: f64 = 5.0;

/// A board tab (2D view).
pub struct Board2dTab {
    id: TabId,
    project: Rc<AppProject>,
    board: BoardId,
    scheme: ColorScheme,
    side: BoardSide,
    scene: Option<BoardScene>,
    /// Journal cursor of the scene.
    sync: SceneSync,
    canvas: CanvasView,
    title: String,
    grid_style: GridStyle,
    grid_interval: PositiveLength,
    unit: LengthUnit,
    /// Board layers listed in the layers panel (same order as the model).
    layers: Vec<Layer>,
    /// Visibility per listed layer.
    visible: Vec<bool>,
    layers_model: Rc<UiModel<ui::GraphicsLayerData>>,
    selected: Option<BoardObject>,
    frame: i32,
    scene_image_pos: slint::LogicalPosition,
}

impl Board2dTab {
    /// Creates the tab of a board.
    pub fn new(project: Rc<AppProject>, board: BoardId, grid_style: GridStyle) -> Self {
        let scheme = ColorScheme::BOARD_DARK;
        let side = BoardSide::Top;
        let (scene, revision, title, grid_interval, unit) = {
            let p = project.shared().lock();
            let proj = p.project();
            let scene = BoardScene::build(proj, board, side, &scheme)
                .map_err(|e| log::error!("Failed to build the board scene: {e}"))
                .ok();
            let b = proj.board(board);
            (
                scene,
                proj.revision(),
                b.map(|b| b.properties().name.to_string())
                    .unwrap_or_default(),
                b.map_or(
                    PositiveLength::new(Length::new(635_000)).expect("positive"),
                    |b| b.settings().grid_interval,
                ),
                b.map_or(LengthUnit::Millimeters, |b| b.settings().grid_unit),
            )
        };
        let background = scene.as_ref().map_or(Color::BLACK, BoardScene::background);
        let content = scene.as_ref().and_then(BoardScene::content_bounds);
        let mut tab = Self {
            id: TabId::new(),
            project,
            board,
            scheme,
            side,
            scene,
            sync: SceneSync::at(revision),
            canvas: CanvasView::new(background, DEFAULT_BOARD_RECT, content),
            title,
            grid_style,
            grid_interval,
            unit,
            layers: Vec::new(),
            visible: Vec::new(),
            layers_model: UiModel::shared(Vec::new()),
            selected: None,
            frame: 0,
            scene_image_pos: Default::default(),
        };
        tab.init_layers();
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

    /// The board shown.
    pub fn board(&self) -> BoardId {
        self.board
    }

    /// The graphics view.
    pub fn canvas_mut(&mut self) -> &mut CanvasView {
        &mut self.canvas
    }

    /// The scene (if it could be built).
    pub fn scene(&self) -> Option<&BoardScene> {
        self.scene.as_ref()
    }

    /// The layers panel model (`TabData.layers`).
    pub fn layers_model(&self) -> &Rc<UiModel<ui::GraphicsLayerData>> {
        &self.layers_model
    }

    /// Lists the board layers of the scene (upstream
    /// `GraphicsLayerList::boardLayers()`, only layers used by the board).
    fn init_layers(&mut self) {
        let Some(scene) = &self.scene else { return };
        let used: Vec<Layer> = scene
            .layers()
            .iter()
            .filter_map(|l| match l {
                BoardSceneLayer::Board(layer) => Some(*layer),
                _ => None,
            })
            .collect();
        self.layers = Layer::all()
            .iter()
            .copied()
            .filter(|l| used.contains(l))
            .collect();
        self.visible = self
            .layers
            .iter()
            .map(|l| {
                scene
                    .scene()
                    .is_layer_visible(BoardSceneLayer::Board(*l).id())
            })
            .collect();
        self.update_layers_model();
    }

    /// Re-lists the layers after a rebuild (the board's layers may have
    /// changed), keeping the visibility of known layers.
    fn init_layers_keep_visibility(&mut self) {
        let old: Vec<(Layer, bool)> = self
            .layers
            .iter()
            .copied()
            .zip(self.visible.iter().copied())
            .collect();
        self.init_layers();
        for (i, layer) in self.layers.iter().enumerate() {
            if let Some((_, v)) = old.iter().find(|(l, _)| l == layer) {
                self.visible[i] = *v;
            }
        }
        if let Some(scene) = &mut self.scene {
            for (layer, visible) in self.layers.iter().zip(&self.visible) {
                scene.set_layer_visible(*layer, *visible);
            }
        }
        self.update_layers_model();
    }

    fn update_layers_model(&self) {
        let Some(scene) = &self.scene else { return };
        let rows = self
            .layers
            .iter()
            .zip(&self.visible)
            .map(|(layer, visible)| {
                let canvas_layer = scene.scene().layer(BoardSceneLayer::Board(*layer).id());
                let (color, highlight) = canvas_layer
                    .map(|l| (l.color, l.highlight_color))
                    .unwrap_or((Color::WHITE, Color::WHITE));
                ui::GraphicsLayerData {
                    name: layer.name_tr().into(),
                    color: color_to_ui(color),
                    color_highlighted: color_to_ui(highlight),
                    visible: *visible,
                }
            })
            .collect();
        self.layers_model.replace_all(rows);
    }

    /// Shows or hides the layer of a layers panel row.
    pub fn set_layer_visible(&mut self, row: usize, visible: bool) -> TabUpdate {
        let Some(layer) = self.layers.get(row).copied() else {
            return TabUpdate::default();
        };
        if self.visible[row] == visible {
            return TabUpdate::default();
        }
        self.visible[row] = visible;
        if let Some(scene) = &mut self.scene {
            scene.set_layer_visible(layer, visible);
        }
        TabUpdate::repaint()
    }

    /// Applies a layer visibility preset (upstream `TabAction::Layers*`).
    fn apply_layer_preset(&mut self, action: ui::TabAction) {
        for (i, layer) in self.layers.iter().enumerate() {
            let inner = layer.is_copper() && !layer.is_top() && !layer.is_bottom();
            self.visible[i] = match action {
                ui::TabAction::LayersNone => false,
                ui::TabAction::LayersTop => !layer.is_bottom() && !inner,
                ui::TabAction::LayersBottom => !layer.is_top() && !inner,
                ui::TabAction::LayersTopBottom => !inner,
                _ => true,
            };
        }
        if let Some(scene) = &mut self.scene {
            for (layer, visible) in self.layers.iter().zip(&self.visible) {
                scene.set_layer_visible(*layer, *visible);
            }
        }
        self.update_layers_model();
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

    /// Sets the grid style (a workspace setting shared by all tabs).
    pub fn set_grid_style(&mut self, style: GridStyle) -> TabUpdate {
        if style == self.grid_style {
            return TabUpdate::default();
        }
        self.grid_style = style;
        self.apply_grid();
        TabUpdate::repaint()
    }

    /// Rebuilds the scene (e.g. after the project changed or the side was
    /// flipped), keeping the layer visibility.
    fn rebuild(&mut self) {
        let p = self.project.shared().lock();
        let proj = p.project();
        match BoardScene::build(proj, self.board, self.side, &self.scheme) {
            Ok(mut scene) => {
                for (layer, visible) in self.layers.iter().zip(&self.visible) {
                    scene.set_layer_visible(*layer, *visible);
                }
                self.scene = Some(scene);
                self.sync = SceneSync::new(proj);
                if let Some(b) = proj.board(self.board) {
                    self.title = b.properties().name.to_string();
                }
            }
            Err(e) => log::error!("Failed to rebuild the board scene: {e}"),
        }
        self.selected = None;
    }

    /// Updates the scene from the project's change journal if the project
    /// changed (e.g. through the MCP server): incrementally where possible.
    pub fn rebuild_if_modified(&mut self) -> TabUpdate {
        let p = self.project.shared().lock();
        let proj = p.project();
        if !self.sync.is_outdated(proj) {
            return TabUpdate::default();
        }
        let result = match &mut self.scene {
            Some(scene) => self.sync.sync(proj, scene),
            None => Ok(true),
        };
        let rebuilt = match result {
            Ok(rebuilt) => rebuilt,
            Err(e) => {
                log::warn!("Incremental board scene update failed, rebuilding: {e}");
                true
            }
        };
        if let Some(b) = proj.board(self.board) {
            self.title = b.properties().name.to_string();
        }
        drop(p);
        if rebuilt {
            self.rebuild();
            self.init_layers_keep_visibility();
        } else {
            self.select(self.selected);
        }
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
            background_image: ui::FeatureState::NotSupported,
            export_graphics: ui::FeatureState::NotSupported,
            select: ui::FeatureState::NotSupported,
            find: ui::FeatureState::NotSupported,
            ..Default::default()
        };
        ui::TabData {
            r#type: ui::TabType::Board2d,
            title: self.title.as_str().into(),
            features,
            read_only: !writable,
            unsaved_changes: p.has_unsaved_changes(),
            undo_text: undo.undo_text().unwrap_or_default().into(),
            redo_text: undo.redo_text().unwrap_or_default().into(),
            layers: model_rc(&self.layers_model),
            ..Default::default()
        }
    }

    /// The `Board2dTabData`.
    pub fn derived_ui_data(&self, projects: &[Rc<AppProject>]) -> ui::Board2dTabData {
        let background = self
            .scene
            .as_ref()
            .map_or(Color::BLACK, BoardScene::background);
        let board_index = {
            let p = self.project.shared().lock();
            p.project().board_index(self.board).map_or(-1, |i| i as i32)
        };
        ui::Board2dTabData {
            project_index: project_index(projects, &self.project),
            board_index,
            background_color: color_to_ui(background).into(),
            foreground_color: slint::Color::from_rgb_u8(255, 255, 255).into(),
            overlay_color: slint::Color::from_argb_u8(0x82, 0, 0, 0).into(),
            overlay_text_color: slint::Color::from_rgb_u8(0xff, 0xff, 0).into(),
            grid_style: grid_style_to_ui(self.grid_style),
            grid_interval: length_to_ui(self.grid_interval.get()),
            unit: unit_to_ui(self.unit),
            flip_view: self.side == BoardSide::Bottom,
            background_image_alpha: 1.0,
            tool: ui::EditorTool::Select,
            unplaced_components_index: -1,
            unplaced_components_devices_index: -1,
            unplaced_components_footprints_index: -1,
            scene_image_pos: self.scene_image_pos,
            frame: self.frame,
            ..Default::default()
        }
    }

    /// Applies `Board2dTabData` written by the UI.
    pub fn set_derived_ui_data(&mut self, data: &ui::Board2dTabData) -> TabUpdate {
        let mut update = TabUpdate::default();
        self.scene_image_pos = data.scene_image_pos;
        let style = grid_style_from_ui(data.grid_style);
        if style != self.grid_style {
            update = self.set_grid_style(style);
        }
        if let Ok(interval) = PositiveLength::new(length_from_ui(data.grid_interval.clone()))
            && interval != self.grid_interval
        {
            self.grid_interval = interval;
            self.apply_grid();
            update.repaint = true;
        }
        let unit = unit_from_ui(data.unit);
        if unit != self.unit {
            self.unit = unit;
            update.data_changed = true;
        }
        let side = if data.flip_view {
            BoardSide::Bottom
        } else {
            BoardSide::Top
        };
        if side != self.side {
            self.side = side;
            self.canvas.set_mirrored(side == BoardSide::Bottom);
            self.rebuild();
            update.repaint = true;
            update.data_changed = true;
        }
        update
    }

    /// Handles a tab action.
    pub fn trigger(&mut self, action: ui::TabAction) -> TabUpdate {
        let content = self.scene.as_ref().and_then(BoardScene::content_bounds);
        match action {
            ui::TabAction::ZoomFit => self.canvas.zoom_fit(content),
            ui::TabAction::ZoomIn => self.canvas.zoom_in(),
            ui::TabAction::ZoomOut => self.canvas.zoom_out(),
            ui::TabAction::LayersNone
            | ui::TabAction::LayersTop
            | ui::TabAction::LayersBottom
            | ui::TabAction::LayersTopBottom
            | ui::TabAction::LayersAll => self.apply_layer_preset(action),
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
            ui::TabAction::Abort | ui::TabAction::ToolSelect => self.select(None),
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

    fn object_at(&self, world: Point) -> Option<BoardObject> {
        let scene = self.scene.as_ref()?;
        let tolerance = self.canvas.view().pixels_to_world(HIT_TOLERANCE_PX);
        let item = scene.scene().hit_test(world, tolerance)?;
        scene.object(item)
    }

    /// Selects all items of an object (or clears the selection).
    fn select(&mut self, object: Option<BoardObject>) {
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
    fn describe(&self, object: BoardObject) -> String {
        let p = self.project.shared().lock();
        let project = p.project();
        match object {
            BoardObject::Device(id) | BoardObject::FootprintPad(id, _) => {
                let name = project
                    .circuit()
                    .component_instance(id)
                    .map(|c| c.properties().name.to_string())
                    .unwrap_or_default();
                if matches!(object, BoardObject::FootprintPad(..)) {
                    tr!("Board2dTab", "Pad of {0}", name)
                } else {
                    tr!("Board2dTab", "Device {0}", name)
                }
            }
            BoardObject::Pad(..) => tr!("Board2dTab", "Pad"),
            BoardObject::Via(..) => tr!("Board2dTab", "Via"),
            BoardObject::Trace(..) => tr!("Board2dTab", "Trace"),
            BoardObject::Plane(_) => tr!("Board2dTab", "Plane"),
            BoardObject::Zone(_) => tr!("Board2dTab", "Zone"),
            BoardObject::Polygon(_) => tr!("Board2dTab", "Polygon"),
            BoardObject::StrokeText(_) => tr!("Board2dTab", "Text"),
            BoardObject::Hole(_) => tr!("Board2dTab", "Hole"),
            BoardObject::AirWire(_) => tr!("Board2dTab", "Air Wire"),
        }
    }

    /// Bumps the frame counter (the UI then requests a new image).
    pub fn bump_frame(&mut self) {
        self.frame = self.frame.wrapping_add(1);
    }
}
