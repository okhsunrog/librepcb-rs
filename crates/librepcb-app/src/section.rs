//! Window sections: a main window is split horizontally into sections, each
//! with its own tab bar.
//!
//! Port of libs/librepcb/editor/windowsection.{h,cpp}. The section's
//! `tabs` model holds the base `TabData` of every tab; the per-kind models
//! (`schematic-tabs`, `board-2d-tabs`, ...) are "proxy views" with the same
//! row count, holding the kind's data for tabs of that kind and default
//! data for the others (upstream `DerivedUiObjectList`). Models of tab
//! kinds which are not ported yet stay empty (indexing them yields default
//! data in Slint).

use std::rc::Rc;

use librepcb_app_ui as ui;

use crate::models::{UiModel, model_rc};
use crate::project::AppProject;
use crate::tabs::{Tab, TabId};

/// The models of a section written by the UI: base tab data, schematic
/// tab data and board tab data.
pub type SectionModels<'a> = (
    &'a Rc<UiModel<ui::TabData>>,
    &'a Rc<UiModel<ui::SchematicTabData>>,
    &'a Rc<UiModel<ui::Board2dTabData>>,
);

/// A window section.
pub struct WindowSection {
    tabs: Vec<Tab>,
    current: i32,
    /// Recently active tabs (last is current), to go back when closing.
    previous: Vec<TabId>,
    highlight: bool,
    tabs_model: Rc<UiModel<ui::TabData>>,
    schematic_model: Rc<UiModel<ui::SchematicTabData>>,
    board_model: Rc<UiModel<ui::Board2dTabData>>,
}

impl Default for WindowSection {
    fn default() -> Self {
        Self::new()
    }
}

impl WindowSection {
    /// Creates an empty section.
    pub fn new() -> Self {
        Self {
            tabs: Vec::new(),
            current: -1,
            previous: Vec::new(),
            highlight: false,
            tabs_model: UiModel::shared(Vec::new()),
            schematic_model: UiModel::shared(Vec::new()),
            board_model: UiModel::shared(Vec::new()),
        }
    }

    /// The `WindowSectionData`.
    pub fn ui_data(&self) -> ui::WindowSectionData {
        ui::WindowSectionData {
            tabs: model_rc(&self.tabs_model),
            schematic_tabs: model_rc(&self.schematic_model),
            board_2d_tabs: model_rc(&self.board_model),
            current_tab_index: self.current,
            highlight: self.highlight,
            ..Default::default()
        }
    }

    /// The models written by the UI (to connect their handlers).
    pub fn models(&self) -> SectionModels<'_> {
        (&self.tabs_model, &self.schematic_model, &self.board_model)
    }

    /// The tabs.
    pub fn tabs(&self) -> &[Tab] {
        &self.tabs
    }

    /// A tab, mutably.
    pub fn tab_mut(&mut self, index: usize) -> Option<&mut Tab> {
        self.tabs.get_mut(index)
    }

    /// The tabs, mutably.
    pub fn tabs_mut(&mut self) -> &mut [Tab] {
        &mut self.tabs
    }

    /// The index of a tab.
    pub fn tab_index(&self, id: TabId) -> Option<usize> {
        self.tabs.iter().position(|t| t.id() == id)
    }

    /// The current tab index (-1 if none).
    pub fn current_index(&self) -> i32 {
        self.current
    }

    /// The current tab.
    pub fn current_tab_mut(&mut self) -> Option<&mut Tab> {
        usize::try_from(self.current)
            .ok()
            .and_then(|i| self.tabs.get_mut(i))
    }

    /// Whether the first tab is the home tab.
    pub fn has_home_tab(&self) -> bool {
        matches!(self.tabs.first(), Some(Tab::Home(_)))
    }

    /// Adds a tab at `index` (end if `None`), optionally making it current.
    pub fn add_tab(
        &mut self,
        tab: Tab,
        index: Option<usize>,
        switch_to: bool,
        projects: &[Rc<AppProject>],
    ) -> usize {
        let index = index.unwrap_or(self.tabs.len()).min(self.tabs.len());
        let mut current = self.current.max(0);
        if !self.tabs.is_empty() && index as i32 <= current {
            current += 1;
        }
        self.tabs_model.insert(index, tab.ui_data());
        self.schematic_model
            .insert(index, tab.schematic_data(projects));
        self.board_model.insert(index, tab.board_data(projects));
        self.tabs.insert(index, tab);
        self.set_current_tab(if switch_to { index as i32 } else { current });
        index
    }

    /// Removes a tab; returns it and whether it was the current tab.
    pub fn remove_tab(&mut self, index: usize) -> Option<(Tab, bool)> {
        if index >= self.tabs.len() {
            return None;
        }
        let was_current = self.current == index as i32;
        let tab = self.tabs.remove(index);
        self.tabs_model.remove(index);
        self.schematic_model.remove(index);
        self.board_model.remove(index);
        self.previous.retain(|id| *id != tab.id());
        let mut current = self.current;
        if was_current {
            if let Some(i) = self
                .previous
                .last()
                .and_then(|id| self.tabs.iter().position(|t| t.id() == *id))
            {
                current = i as i32;
            }
        } else if current > index as i32 {
            current -= 1;
        }
        self.current = -2; // Force the update.
        self.set_current_tab(current.min(self.tabs.len() as i32 - 1));
        Some((tab, was_current))
    }

    /// Makes a tab current; returns whether it changed.
    pub fn set_current_tab(&mut self, index: i32) -> bool {
        if index == self.current {
            return false;
        }
        self.current = index;
        if let Some(tab) = usize::try_from(index).ok().and_then(|i| self.tabs.get(i)) {
            let id = tab.id();
            self.previous.retain(|p| *p != id);
            self.previous.push(id);
        }
        true
    }

    /// Updates the model rows of a tab (after its data changed).
    pub fn refresh_tab(&self, index: usize, projects: &[Rc<AppProject>]) {
        if let Some(tab) = self.tabs.get(index) {
            self.tabs_model.set(index, tab.ui_data());
            self.schematic_model
                .set(index, tab.schematic_data(projects));
            self.board_model.set(index, tab.board_data(projects));
        }
    }

    /// Updates only the per-kind rows of a tab (e.g. the frame counter).
    pub fn refresh_derived(&self, index: usize, projects: &[Rc<AppProject>]) {
        if let Some(tab) = self.tabs.get(index) {
            match tab {
                Tab::Schematic(_) => self
                    .schematic_model
                    .set(index, tab.schematic_data(projects)),
                Tab::Board2d(_) => self.board_model.set(index, tab.board_data(projects)),
                Tab::Home(_) => {}
            }
        }
    }

    /// Updates the model rows of all tabs.
    pub fn refresh_all(&self, projects: &[Rc<AppProject>]) {
        for i in 0..self.tabs.len() {
            self.refresh_tab(i, projects);
        }
    }

    /// Sets the highlight flag (upstream highlights a section for one
    /// second when a tab is opened in it).
    pub fn set_highlight(&mut self, highlight: bool) {
        self.highlight = highlight;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_remove_tabs() {
        let mut s = WindowSection::new();
        s.add_tab(Tab::Home(TabId::new()), None, false, &[]);
        assert_eq!(s.current_index(), 0);
        let a = TabId::new();
        s.add_tab(Tab::Home(a), None, true, &[]);
        assert_eq!(s.current_index(), 1);
        let b = TabId::new();
        s.add_tab(Tab::Home(b), Some(0), false, &[]);
        // Inserted before the current tab: the current index follows.
        assert_eq!(s.current_index(), 2);
        assert_eq!(s.tab_index(a), Some(2));
        assert_eq!(s.models().0.len(), 3);
        assert_eq!(s.models().1.len(), 3);
        s.set_current_tab(0);
        // Closing the current tab goes back to the previous one.
        let (tab, was_current) = s.remove_tab(0).unwrap();
        assert_eq!(tab.id(), b);
        assert!(was_current);
        assert_eq!(s.current_index(), 1);
        assert_eq!(s.tab_index(a), Some(1));
        assert_eq!(s.models().2.len(), 2);
    }
}
