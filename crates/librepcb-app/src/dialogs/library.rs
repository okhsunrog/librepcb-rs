//! Dialogs of the library tab: the confirmation before removing library
//! elements (upstream: a `QMessageBox::warning()` of
//! `LibraryTab::deleteElements()`).

use librepcb_core::fileio::FilePath;
use librepcb_core::types::LengthUnit;
use librepcb_i18n::tr;

use super::{Applied, DialogContext, DialogOptions, Form, FormDialog, TabDialogResult};

const LT: &str = "librepcb::editor::LibraryTab";

/// Asks whether library elements shall really be removed (upstream
/// `LibraryTab::deleteElements()`).
pub struct RemoveElementsDialog {
    form: Form,
    paths: Vec<FilePath>,
}

impl RemoveElementsDialog {
    /// A dialog for the elements (directory, name).
    pub fn new(elements: &[(FilePath, String)]) -> Self {
        let mut names: Vec<String> = elements.iter().map(|(_, n)| n.clone()).collect();
        super::sort_numeric(&mut names);
        let mut msg = tr!(
            LT,
            "WARNING: Library elements must normally NOT be removed because this will break other elements which depend on this one! They should be just marked as deprecated instead.\n\nAre you still sure to delete the following library elements?"
        );
        msg.push_str("\n\n");
        for name in names.iter().take(10) {
            msg.push_str(&format!(" - {name}\n"));
        }
        if names.len() > 10 {
            msg.push_str(" - ...\n");
        }
        msg.push('\n');
        msg.push_str(&tr!(LT, "This cannot be undone!"));
        let mut form = Form::new(LengthUnit::Millimeters);
        form.note("message", msg);
        Self {
            form,
            paths: elements.iter().map(|(p, _)| p.clone()).collect(),
        }
    }
}

impl FormDialog for RemoveElementsDialog {
    fn title(&self) -> String {
        tr!(LT, "Remove {0} Elements", self.paths.len())
    }

    form_accessors!();

    fn options(&self) -> DialogOptions {
        DialogOptions {
            apply: false,
            ok_text: Some(tr!("QDialogButtonBox", "&Yes").replace('&', "")),
            width: 500.0,
            label_width: 0.0,
            ..DialogOptions::default()
        }
    }

    fn apply(&mut self, _ctx: &DialogContext<'_>) -> Result<Applied, String> {
        Ok(Applied::Tab(TabDialogResult::RemoveLibraryElements(
            self.paths.clone(),
        )))
    }
}
