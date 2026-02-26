use super::ChatExit;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Screen {
    Chat,
    Threads,
    Assistants,
    Runs,
    Store,
    Crons,
    Deployments,
}

impl Screen {
    pub fn label(&self) -> &str {
        match self {
            Screen::Chat => "Chat",
            Screen::Threads => "Threads",
            Screen::Assistants => "Assistants",
            Screen::Runs => "Runs",
            Screen::Store => "Store",
            Screen::Crons => "Crons",
            Screen::Deployments => "Deployments",
        }
    }

    pub fn all() -> &'static [Screen] {
        &[
            Screen::Chat,
            Screen::Threads,
            Screen::Assistants,
            Screen::Runs,
            Screen::Store,
            Screen::Crons,
            Screen::Deployments,
        ]
    }

    /// Fuzzy match screen name from user input (e.g. "th" -> Threads)
    pub fn from_input(input: &str) -> Option<Screen> {
        let lower = input.to_lowercase();
        Screen::all()
            .iter()
            .find(|s| s.label().to_lowercase().starts_with(&lower))
            .cloned()
    }
}

/// Context passed when navigating between screens
#[derive(Debug, Clone)]
pub enum ScreenContext {
    Thread(String),
    Assistant(String),
    SwitchContext(String),
}

pub enum ScreenAction {
    None,
    Navigate(Screen),
    NavigateWithContext(Screen, ScreenContext),
    #[allow(dead_code)]
    Back,
    Quit,
    #[allow(dead_code)]
    Refresh,
    ChatExit(ChatExit),
}
