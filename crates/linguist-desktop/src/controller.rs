#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ServiceState {
    Checking,
    Ready,
    Unavailable(String),
}

impl ServiceState {
    pub fn label(&self) -> String {
        match self {
            Self::Checking => "Checking".into(),
            Self::Ready => "Ready".into(),
            Self::Unavailable(message) if message.is_empty() => "Unavailable".into(),
            Self::Unavailable(message) => format!("Unavailable: {message}"),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ViewState {
    pub anki: ServiceState,
    pub ollama: ServiceState,
    pub active_deck: String,
    pub selection: String,
    pub busy: bool,
    pub error: String,
}

impl Default for ViewState {
    fn default() -> Self {
        Self {
            anki: ServiceState::Checking,
            ollama: ServiceState::Checking,
            active_deck: String::new(),
            selection: String::new(),
            busy: false,
            error: String::new(),
        }
    }
}

pub trait DesktopPort {
    fn anki_available(&self) -> Result<(), String>;
    fn ollama_available(&self) -> Result<(), String>;
    fn active_deck(&self) -> Result<String, String>;
}

#[derive(Clone, Debug, Default)]
pub struct ApplicationController {
    state: ViewState,
}

impl ApplicationController {
    pub fn state(&self) -> &ViewState {
        &self.state
    }

    pub fn refresh<P: DesktopPort>(&mut self, port: &P) {
        self.state.busy = true;
        self.state.error.clear();
        self.state.anki = service_state(port.anki_available());
        self.state.ollama = service_state(port.ollama_available());
        match port.active_deck() {
            Ok(deck) => self.state.active_deck = deck,
            Err(error) => {
                self.state.active_deck.clear();
                self.state.error = error;
            }
        }
        self.state.busy = false;
    }

    pub fn select(&mut self, selection: impl Into<String>) {
        self.state.selection = selection.into();
    }

    pub fn report_error(&mut self, error: impl Into<String>) {
        self.state.error = error.into();
    }

    pub fn clear_error(&mut self) {
        self.state.error.clear();
    }
}

fn service_state(result: Result<(), String>) -> ServiceState {
    match result {
        Ok(()) => ServiceState::Ready,
        Err(error) => ServiceState::Unavailable(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakePort {
        anki: Result<(), String>,
        ollama: Result<(), String>,
        deck: Result<String, String>,
    }

    impl DesktopPort for FakePort {
        fn anki_available(&self) -> Result<(), String> {
            self.anki.clone()
        }
        fn ollama_available(&self) -> Result<(), String> {
            self.ollama.clone()
        }
        fn active_deck(&self) -> Result<String, String> {
            self.deck.clone()
        }
    }

    #[test]
    fn refresh_exposes_service_deck_and_error_state_through_one_snapshot() {
        let mut controller = ApplicationController::default();
        controller.refresh(&FakePort {
            anki: Ok(()),
            ollama: Err("offline".into()),
            deck: Err("Anki unavailable".into()),
        });
        assert_eq!(controller.state().anki, ServiceState::Ready);
        assert_eq!(controller.state().ollama.label(), "Unavailable: offline");
        assert_eq!(controller.state().error, "Anki unavailable");
        assert!(!controller.state().busy);
    }

    #[test]
    fn selection_and_error_commands_keep_view_state_immutable_to_callers() {
        let mut controller = ApplicationController::default();
        controller.select("42 · 食べる");
        controller.report_error("invalid model");
        assert_eq!(controller.state().selection, "42 · 食べる");
        controller.clear_error();
        assert!(controller.state().error.is_empty());
    }
}
