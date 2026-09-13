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
        #[qproperty(QString, queue_state)]
        #[qproperty(QString, queue_message)]
        #[qproperty(i32, deck_count)]
        #[qproperty(i32, selected_deck_index)]
        #[qproperty(i32, review_row_count)]
        #[qproperty(i32, selected_review_index)]
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
        #[qinvokable]
        #[cxx_name = "deckName"]
        fn deck_name(self: &AppBackend, index: i32) -> QString;
        #[qinvokable]
        #[cxx_name = "selectDeckIndex"]
        fn select_deck_index(self: Pin<&mut Self>, index: i32);
        #[qinvokable]
        #[cxx_name = "reviewExpression"]
        fn review_expression(self: &AppBackend, index: i32) -> QString;
        #[qinvokable]
        #[cxx_name = "reviewDetail"]
        fn review_detail(self: &AppBackend, index: i32) -> QString;
        #[qinvokable]
        #[cxx_name = "reviewState"]
        fn review_state(self: &AppBackend, index: i32) -> QString;
        #[qinvokable]
        #[cxx_name = "selectReviewIndex"]
        fn select_review_index(self: Pin<&mut Self>, index: i32);
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
    queue_state: QString,
    queue_message: QString,
    deck_count: i32,
    selected_deck_index: i32,
    review_row_count: i32,
    selected_review_index: i32,
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
            queue_state: controller.queue().state().label().into(),
            queue_message: controller.queue().state().message().into(),
            deck_count: queue_len(controller.queue().decks().len()),
            selected_deck_index: queue_index(controller.queue().selected_deck_index()),
            review_row_count: queue_len(controller.queue().rows().len()),
            selected_review_index: queue_index(controller.queue().selected_index()),
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

    pub fn deck_name(&self, index: i32) -> QString {
        self.rust()
            .controller
            .queue()
            .decks()
            .get(index as usize)
            .cloned()
            .unwrap_or_default()
            .into()
    }

    pub fn select_deck_index(mut self: Pin<&mut Self>, index: i32) {
        if let Ok(index) = usize::try_from(index) {
            self.as_mut().rust_mut().controller.select_deck_index(index);
            sync_controller_state(self);
        }
    }

    pub fn review_expression(&self, index: i32) -> QString {
        review_row_value(self, index, |row| &row.expression)
    }

    pub fn review_detail(&self, index: i32) -> QString {
        review_row_value(self, index, |row| &row.detail)
    }

    pub fn review_state(&self, index: i32) -> QString {
        review_row_value(self, index, |row| row.state.label())
    }

    pub fn select_review_index(mut self: Pin<&mut Self>, index: i32) {
        if let Ok(index) = usize::try_from(index) {
            self.as_mut()
                .rust_mut()
                .controller
                .select_queue_index(index);
            sync_controller_state(self);
        }
    }
}

fn sync_controller_state(mut qobject: Pin<&mut qobject::AppBackend>) {
    let state = qobject.as_ref().rust().controller.state().clone();
    let (
        queue_state,
        queue_message,
        deck_count,
        selected_deck_index,
        review_row_count,
        selected_review_index,
    ) = {
        let binding = qobject.as_ref();
        let queue = binding.rust().controller.queue();
        (
            queue.state().label(),
            queue.state().message(),
            queue_len(queue.decks().len()),
            queue_index(queue.selected_deck_index()),
            queue_len(queue.rows().len()),
            queue_index(queue.selected_index()),
        )
    };
    qobject.as_mut().set_anki_status(state.anki.label().into());
    qobject
        .as_mut()
        .set_ollama_status(state.ollama.label().into());
    qobject.as_mut().set_active_deck(state.active_deck.into());
    qobject.as_mut().set_selection(state.selection.into());
    qobject.as_mut().set_error_message(state.error.into());
    qobject.as_mut().set_busy(state.busy);
    qobject.as_mut().set_queue_state(queue_state.into());
    qobject.as_mut().set_queue_message(queue_message.into());
    qobject.as_mut().set_deck_count(deck_count);
    qobject
        .as_mut()
        .set_selected_deck_index(selected_deck_index);
    qobject.as_mut().set_review_row_count(review_row_count);
    qobject.set_selected_review_index(selected_review_index);
}

fn review_row_value(
    qobject: &qobject::AppBackend,
    index: i32,
    value: impl FnOnce(&crate::review_model::ReviewRow) -> &str,
) -> QString {
    usize::try_from(index)
        .ok()
        .and_then(|index| qobject.rust().controller.queue().rows().get(index))
        .map(value)
        .unwrap_or_default()
        .into()
}

fn queue_len(len: usize) -> i32 {
    len.try_into().unwrap_or(i32::MAX)
}

fn queue_index(index: Option<usize>) -> i32 {
    index.and_then(|index| index.try_into().ok()).unwrap_or(-1)
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
    fn review_queue(&self) -> Result<crate::review_model::ReviewQueueData, String> {
        Err("Connect Anki to load review cards".into())
    }
}
