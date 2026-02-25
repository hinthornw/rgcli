use super::ChatExit;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Screen {
    Chat,
    Threads,
    Assistants,
    Runs,
    Store,
    Crons,
    Logs,
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
            Screen::Logs => "Logs",
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
            Screen::Logs,
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
    #[allow(dead_code)]
    ThreadRuns(String),
    #[allow(dead_code)]
    RunDetail(String, String),
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
