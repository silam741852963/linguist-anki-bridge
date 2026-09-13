#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(QString, theme_background)]
        #[qproperty(QString, theme_surface)]
        #[qproperty(QString, theme_foreground)]
        #[qproperty(QString, theme_muted)]
        #[qproperty(QString, theme_accent)]
        #[qproperty(QString, anki_status)]
        #[qproperty(QString, ollama_status)]
        #[qproperty(QString, active_deck)]
        #[qproperty(QString, selection)]
        #[qproperty(QString, error_message)]
        #[qproperty(bool, busy)]
        #[namespace = "linguist"]
        type AppBackend = super::AppBackendRust;

        #[qinvokable]
        #[cxx_name = "reloadTheme"]
        fn reload_theme(self: Pin<&mut Self>);
        #[qinvokable]
        #[cxx_name = "refreshState"]
        fn refresh_state(self: Pin<&mut Self>);
        #[qinvokable]
        #[cxx_name = "clearError"]
        fn clear_error(self: Pin<&mut Self>);
        #[qinvokable]
        #[cxx_name = "selectItem"]
        fn select_item(self: Pin<&mut Self>, selection: &QString);
        #[qinvokable]
        #[cxx_name = "reportError"]
        fn report_error(self: Pin<&mut Self>, message: &QString);
    }
}

use std::pin::Pin;

use cxx_qt::CxxQtType;
use cxx_qt_lib::QString;

use crate::controller::{ApplicationController, DesktopPort};
use crate::theme::ThemePalette;

pub struct AppBackendRust {
    theme_background: QString,
    theme_surface: QString,
    theme_foreground: QString,
    theme_muted: QString,
    theme_accent: QString,
    anki_status: QString,
    ollama_status: QString,
    active_deck: QString,
    selection: QString,
    error_message: QString,
    busy: bool,
    controller: ApplicationController,
}

impl Default for AppBackendRust {
    fn default() -> Self {
        Self::from_palette(ThemePalette::load())
    }
}

impl AppBackendRust {
    fn from_palette(palette: ThemePalette) -> Self {
        let controller = ApplicationController::default();
        let state = controller.state();
        Self {
            theme_background: palette.background.into(),
            theme_surface: palette.surface.into(),
            theme_foreground: palette.foreground.into(),
            theme_muted: palette.muted.into(),
            theme_accent: palette.accent.into(),
            anki_status: state.anki.label().into(),
            ollama_status: state.ollama.label().into(),
            active_deck: state.active_deck.clone().into(),
            selection: state.selection.clone().into(),
            error_message: state.error.clone().into(),
            busy: state.busy,
            controller,
        }
    }
}

impl qobject::AppBackend {
    pub fn reload_theme(mut self: Pin<&mut Self>) {
        let palette = ThemePalette::load();
        self.as_mut()
            .set_theme_background(palette.background.into());
        self.as_mut().set_theme_surface(palette.surface.into());
        self.as_mut()
            .set_theme_foreground(palette.foreground.into());
        self.as_mut().set_theme_muted(palette.muted.into());
        self.set_theme_accent(palette.accent.into());
    }

    pub fn refresh_state(mut self: Pin<&mut Self>) {
        self.as_mut()
            .rust_mut()
            .controller
            .refresh(&DisconnectedPort);
        sync_controller_state(self);
    }

    pub fn clear_error(mut self: Pin<&mut Self>) {
        self.as_mut().rust_mut().controller.clear_error();
        sync_controller_state(self);
    }

    pub fn select_item(mut self: Pin<&mut Self>, selection: &QString) {
        self.as_mut()
            .rust_mut()
            .controller
            .select(selection.to_string());
        sync_controller_state(self);
    }

    pub fn report_error(mut self: Pin<&mut Self>, message: &QString) {
        self.as_mut()
            .rust_mut()
            .controller
            .report_error(message.to_string());
        sync_controller_state(self);
    }
}

fn sync_controller_state(mut qobject: Pin<&mut qobject::AppBackend>) {
    let state = qobject.as_ref().rust().controller.state().clone();
    qobject.as_mut().set_anki_status(state.anki.label().into());
    qobject
        .as_mut()
        .set_ollama_status(state.ollama.label().into());
    qobject.as_mut().set_active_deck(state.active_deck.into());
    qobject.as_mut().set_selection(state.selection.into());
    qobject.as_mut().set_error_message(state.error.into());
    qobject.set_busy(state.busy);
}

struct DisconnectedPort;

impl DesktopPort for DisconnectedPort {
    fn anki_available(&self) -> Result<(), String> {
        Err("adapter not connected".into())
    }
    fn ollama_available(&self) -> Result<(), String> {
        Err("adapter not connected".into())
    }
    fn active_deck(&self) -> Result<String, String> {
        Err("Connect Anki to load its active deck".into())
    }
}
