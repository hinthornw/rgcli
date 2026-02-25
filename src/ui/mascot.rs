use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

// Kawaii parrot palette — soft, rounded feel
const GREEN: Color = Color::Rgb(100, 210, 120);
const GREEN_LT: Color = Color::Rgb(150, 230, 160);
const BELLY: Color = Color::Rgb(255, 250, 220);
const BEAK: Color = Color::Rgb(255, 180, 80);
const BLUSH: Color = Color::Rgb(255, 150, 150);
const EYE_BG: Color = Color::Rgb(40, 40, 60);
const EYE_SHINE: Color = Color::Rgb(255, 255, 255);
const CREST: Color = Color::Rgb(255, 120, 100);
const WING: Color = Color::Rgb(80, 180, 220);
const FEET: Color = Color::Rgb(255, 180, 80);
const BG: Color = Color::Reset;

fn s(fg_c: Color, bg_c: Color) -> Style {
    Style::default().fg(fg_c).bg(bg_c)
}

fn fg(c: Color) -> Style {
    Style::default().fg(c)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParrotState {
    Idle,
    Typing,
    Thinking,
    Threads,
    Assistants,
    Runs,
    Store,
    Crons,
    Logs,
}

pub struct Parrot {
    state: ParrotState,
    frame: usize,
    tick: usize,
}

impl Parrot {
    pub fn new() -> Self {
        Self {
            state: ParrotState::Idle,
            frame: 0,
            tick: 0,
        }
    }

    pub fn set_state(&mut self, state: ParrotState) {
        if std::mem::discriminant(&self.state) != std::mem::discriminant(&state) {
            self.state = state;
            self.frame = 0;
            self.tick = 0;
        }
    }

    pub fn tick(&mut self) {
        self.tick += 1;
        let speed = match self.state {
            ParrotState::Thinking => 4,
            ParrotState::Idle => 20,
            ParrotState::Runs => 5,
            _ => 10,
        };
        if self.tick % speed == 0 {
            self.frame += 1;
        }
    }

    pub fn render(&self) -> Vec<Line<'static>> {
        match self.state {
            ParrotState::Idle => self.render_idle(),
            ParrotState::Typing => self.render_typing(),
            ParrotState::Thinking => self.render_thinking(),
            ParrotState::Threads => self.render_screen("~"),
            ParrotState::Assistants => self.render_screen("*"),
            ParrotState::Runs => self.render_runs(),
            ParrotState::Store => self.render_thinking(),
            ParrotState::Crons => self.render_screen("@"),
            ParrotState::Logs => self.render_screen("#"),
        }
    }

    fn render_idle(&self) -> Vec<Line<'static>> {
        let blink = self.frame % 15 == 0;
        if blink {
            kawaii_parrot(Eyes::Happy)
        } else {
            kawaii_parrot(Eyes::Normal)
        }
    }

    fn render_typing(&self) -> Vec<Line<'static>> {
        if self.frame % 2 == 0 {
            kawaii_parrot(Eyes::LookRight)
        } else {
            kawaii_parrot(Eyes::LookLeft)
        }
    }

    fn render_thinking(&self) -> Vec<Line<'static>> {
        let eyes = if self.frame % 4 == 3 {
            Eyes::Happy
        } else {
            Eyes::Sparkle
        };
        let dots = match self.frame % 4 {
            0 => " .",
            1 => " ..",
            2 => " ...",
            _ => "",
        };
        let mut lines = kawaii_parrot(eyes);
        if !dots.is_empty() {
            if let Some(line) = lines.get_mut(1) {
                line.spans.push(Span::styled(
                    dots.to_string(),
                    Style::default().fg(Color::Rgb(200, 200, 200)),
                ));
            }
        }
        lines
    }

    fn render_runs(&self) -> Vec<Line<'static>> {
        // Alternate between normal and happy — bouncy energy
        if self.frame % 2 == 0 {
            kawaii_parrot(Eyes::Sparkle)
        } else {
            kawaii_parrot(Eyes::Happy)
        }
    }

    fn render_screen(&self, _marker: &str) -> Vec<Line<'static>> {
        kawaii_parrot(Eyes::Normal)
    }
}

#[derive(Clone, Copy)]
enum Eyes {
    Normal,  // big round eyes with shine
    Happy,   // ^_^ squint
    Sparkle, // star eyes
    LookLeft,
    LookRight,
}

/// Kawaii parrot — 8 lines tall, round and soft
///
/// Design (conceptual):
///    ╭──╮        <- crest tuft
///   ╭(●ω●)╮     <- round head, big eyes, tiny beak
///   │ >//< │     <- blush cheeks
///   ╰┬────┬╯     <- round body
///    ╰┘  ╰┘     <- tiny feet
///
/// Using half-blocks and unicode for soft round shapes.
fn kawaii_parrot(eyes: Eyes) -> Vec<Line<'static>> {
    // Line 0: little crest tuft
    let line0 = Line::from(vec![Span::raw("    "), Span::styled("▄▄", s(CREST, BG))]);

    // Line 1: top of head — round
    let line1 = Line::from(vec![
        Span::raw("  "),
        Span::styled("▄", s(GREEN, BG)),
        Span::styled("▄▄▄▄", s(GREEN_LT, BG)),
        Span::styled("▄", s(GREEN, BG)),
    ]);

    // Line 2: eyes row
    let (le, re) = match eyes {
        Eyes::Normal => (
            Span::styled("●", s(EYE_SHINE, EYE_BG)),
            Span::styled("●", s(EYE_SHINE, EYE_BG)),
        ),
        Eyes::Happy => (
            Span::styled("^", s(EYE_BG, GREEN_LT)),
            Span::styled("^", s(EYE_BG, GREEN_LT)),
        ),
        Eyes::Sparkle => (
            Span::styled("*", s(Color::Rgb(255, 220, 100), EYE_BG)),
            Span::styled("*", s(Color::Rgb(255, 220, 100), EYE_BG)),
        ),
        Eyes::LookLeft => (
            Span::styled("◐", s(EYE_SHINE, EYE_BG)),
            Span::styled("◐", s(EYE_SHINE, EYE_BG)),
        ),
        Eyes::LookRight => (
            Span::styled("◑", s(EYE_SHINE, EYE_BG)),
            Span::styled("◑", s(EYE_SHINE, EYE_BG)),
        ),
    };
    let line2 = Line::from(vec![
        Span::raw(" "),
        Span::styled("▐", fg(GREEN)),
        Span::styled(" ", s(GREEN_LT, GREEN_LT)),
        le,
        Span::styled(" ", s(GREEN_LT, GREEN_LT)),
        re,
        Span::styled(" ", s(GREEN_LT, GREEN_LT)),
        Span::styled("▌", fg(GREEN)),
    ]);

    // Line 3: beak + blush
    let line3 = Line::from(vec![
        Span::raw(" "),
        Span::styled("▐", fg(GREEN)),
        Span::styled(".", s(BLUSH, GREEN_LT)),
        Span::styled(" ", s(GREEN_LT, GREEN_LT)),
        Span::styled("▾", s(BEAK, GREEN_LT)),
        Span::styled(" ", s(GREEN_LT, GREEN_LT)),
        Span::styled(".", s(BLUSH, GREEN_LT)),
        Span::styled("▌", fg(GREEN)),
    ]);

    // Line 4: body top with tiny wings
    let line4 = Line::from(vec![
        Span::styled("~", fg(WING)),
        Span::styled("▐", fg(GREEN)),
        Span::styled("▄▄▄▄▄▄", s(BELLY, GREEN_LT)),
        Span::styled("▌", fg(GREEN)),
        Span::styled("~", fg(WING)),
    ]);

    // Line 5: round belly
    let line5 = Line::from(vec![
        Span::raw(" "),
        Span::styled("▐", fg(GREEN)),
        Span::styled("      ", s(BELLY, BELLY)),
        Span::styled("▌", fg(GREEN)),
    ]);

    // Line 6: bottom
    let line6 = Line::from(vec![Span::raw("  "), Span::styled("▀▀▀▀▀▀", s(GREEN, BG))]);

    // Line 7: tiny feet
    let line7 = Line::from(vec![
        Span::raw("   "),
        Span::styled("▀▘", fg(FEET)),
        Span::raw("  "),
        Span::styled("▀▘", fg(FEET)),
    ]);

    vec![line0, line1, line2, line3, line4, line5, line6, line7]
}

/// Render the parrot for the welcome/logo area, with info text beside it
pub fn logo_with_parrot(
    version: &str,
    endpoint: &str,
    config_path: &str,
    context_info: &str,
    deploy_info: Option<&str>,
) -> Vec<Line<'static>> {
    let parrot_lines = kawaii_parrot(Eyes::Normal);

    let title_style = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);
    let info_style = Style::default()
        .fg(Color::DarkGray)
        .add_modifier(Modifier::ITALIC);

    let info_texts: Vec<Option<Vec<Span<'static>>>> = vec![
        None,
        Some(vec![
            Span::styled("   ailsd", title_style),
            Span::raw(" "),
            Span::styled(version.to_string(), info_style),
        ]),
        Some(vec![Span::styled(format!("   {endpoint}"), info_style)]),
        Some(vec![Span::styled(format!("   {context_info}"), info_style)]),
        Some(vec![Span::styled(format!("   {config_path}"), info_style)]),
        deploy_info.map(|info| vec![Span::styled(format!("   {info}"), info_style)]),
        None,
        None,
    ];

    let mut result = Vec::new();
    for (i, parrot_line) in parrot_lines.into_iter().enumerate() {
        let mut spans = parrot_line.spans;
        if let Some(Some(info_spans)) = info_texts.get(i) {
            spans.extend(info_spans.clone());
        }
        result.push(Line::from(spans));
    }
    result.push(Line::default());
    result
}
