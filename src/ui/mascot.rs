use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

// Kawaii parrot palette
const GREEN: Color = Color::Rgb(90, 200, 110);
const GREEN_LT: Color = Color::Rgb(140, 225, 155);
const GREEN_DK: Color = Color::Rgb(60, 160, 80);
const BELLY: Color = Color::Rgb(245, 245, 215);
const BEAK: Color = Color::Rgb(255, 170, 60);
const BLUSH: Color = Color::Rgb(255, 130, 140);
const EYE_BG: Color = Color::Rgb(30, 30, 50);
const EYE_SHINE: Color = Color::Rgb(255, 255, 255);
const CREST: Color = Color::Rgb(255, 100, 90);
const WING: Color = Color::Rgb(70, 170, 210);
const FEET: Color = Color::Rgb(255, 170, 60);
const BG: Color = Color::Reset;

/// ▄ = fg on bottom half, bg on top half
fn hb(top: Color, bot: Color) -> Style {
    Style::default().fg(bot).bg(top)
}

fn fg(c: Color) -> Style {
    Style::default().fg(c)
}

fn on(fg_c: Color, bg_c: Color) -> Style {
    Style::default().fg(fg_c).bg(bg_c)
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
            ParrotState::Threads => kawaii_parrot(Eyes::Normal),
            ParrotState::Assistants => kawaii_parrot(Eyes::Normal),
            ParrotState::Runs => self.render_runs(),
            ParrotState::Store => self.render_thinking(),
            ParrotState::Crons => kawaii_parrot(Eyes::Normal),
            ParrotState::Logs => kawaii_parrot(Eyes::Normal),
        }
    }

    fn render_idle(&self) -> Vec<Line<'static>> {
        if self.frame % 15 == 0 {
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
        if self.frame % 2 == 0 {
            kawaii_parrot(Eyes::Sparkle)
        } else {
            kawaii_parrot(Eyes::Happy)
        }
    }
}

#[derive(Clone, Copy)]
enum Eyes {
    Normal,
    Happy,
    Sparkle,
    LookLeft,
    LookRight,
}

// Sprite layout (each char = 1 cell, each line = 1 terminal row using half-blocks):
//
//        ▄█▄             crest tuft (line 0)
//      ▄█████▄           rounded head top (line 1)
//     █ ◕   ◕ █          eyes with space (line 2)
//     █ ◗◖    █          beak — wider, visible (line 3)  [note: ◗◖ don't render, use ▼ or text]
//     █ ·   · █          blush cheeks (line 3 alt)
//     ▀▄█████▄▀          wing tips + body transition (line 4) -- nah
//      ▐█████▌           round belly (line 5)
//       ▀▀▀▀▀            bottom curve (line 6)
//        ▪ ▪             tiny feet (line 7)
//
// Actual approach: 9-wide head, use ▄▀ for rounding, colored backgrounds

fn kawaii_parrot(eyes: Eyes) -> Vec<Line<'static>> {
    // Line 0: crest — small rounded tuft
    //    ▄█▄
    let line0 = Line::from(vec![
        Span::raw("    "),
        Span::styled("▄", hb(BG, CREST)),
        Span::styled("█", on(CREST, CREST)),
        Span::styled("▄", hb(BG, CREST)),
    ]);

    // Line 1: top of head — rounded with ▄ half-blocks
    //   ▄███████▄
    let line1 = Line::from(vec![
        Span::raw("  "),
        Span::styled("▄", hb(BG, GREEN_LT)),
        Span::styled("▄", hb(BG, GREEN_LT)),
        Span::styled("▄▄▄▄▄", hb(CREST, GREEN_LT)),
        Span::styled("▄", hb(BG, GREEN_LT)),
        Span::styled("▄", hb(BG, GREEN_LT)),
    ]);

    // Line 2: eyes — big round eyes with spacing
    //  ▐ ◕   ◕ ▌
    let (le, re) = match eyes {
        Eyes::Normal => (
            Span::styled("◕", on(EYE_SHINE, GREEN_LT)),
            Span::styled("◕", on(EYE_SHINE, GREEN_LT)),
        ),
        Eyes::Happy => (
            Span::styled("◡", on(EYE_BG, GREEN_LT)),
            Span::styled("◡", on(EYE_BG, GREEN_LT)),
        ),
        Eyes::Sparkle => (
            Span::styled("✦", on(Color::Rgb(255, 220, 100), GREEN_LT)),
            Span::styled("✦", on(Color::Rgb(255, 220, 100), GREEN_LT)),
        ),
        Eyes::LookLeft => (
            Span::styled("◑", on(EYE_SHINE, GREEN_LT)),
            Span::styled("◑", on(EYE_SHINE, GREEN_LT)),
        ),
        Eyes::LookRight => (
            Span::styled("◐", on(EYE_SHINE, GREEN_LT)),
            Span::styled("◐", on(EYE_SHINE, GREEN_LT)),
        ),
    };
    let g = on(GREEN_LT, GREEN_LT);
    let line2 = Line::from(vec![
        Span::raw(" "),
        Span::styled("▐", fg(GREEN_DK)),
        Span::styled(" ", g),
        le,
        Span::styled("   ", g),
        re,
        Span::styled(" ", g),
        Span::styled("▌", fg(GREEN_DK)),
    ]);

    // Line 3: blush + beak — visible mouth using ▼ or ᴡ
    //  ▐ ◦ ▼ ◦ ▌
    let line3 = Line::from(vec![
        Span::raw(" "),
        Span::styled("▐", fg(GREEN_DK)),
        Span::styled(" ", g),
        Span::styled("◦", on(BLUSH, GREEN_LT)),
        Span::styled(" ", g),
        Span::styled("▼", on(BEAK, GREEN_LT)),
        Span::styled(" ", g),
        Span::styled("◦", on(BLUSH, GREEN_LT)),
        Span::styled(" ", g),
        Span::styled("▌", fg(GREEN_DK)),
    ]);

    // Line 4: neck/body transition with wings
    //  ▐▄███████▄▌
    let line4 = Line::from(vec![
        Span::styled("▗", fg(WING)),
        Span::styled("▐", fg(GREEN_DK)),
        Span::styled("▄", hb(GREEN_LT, BELLY)),
        Span::styled("▄▄▄▄▄", hb(GREEN_LT, BELLY)),
        Span::styled("▄", hb(GREEN_LT, BELLY)),
        Span::styled("▌", fg(GREEN_DK)),
        Span::styled("▖", fg(WING)),
    ]);

    // Line 5: belly
    //  ▐       ▌
    let line5 = Line::from(vec![
        Span::raw(" "),
        Span::styled("▐", fg(GREEN_DK)),
        Span::styled("       ", on(BELLY, BELLY)),
        Span::styled("▌", fg(GREEN_DK)),
    ]);

    // Line 6: bottom — rounded
    //   ▀▀▀▀▀▀▀
    let line6 = Line::from(vec![
        Span::raw("  "),
        Span::styled("▀", hb(BG, GREEN_DK)),
        Span::styled("▀▀▀▀▀", hb(BG, BELLY)),
        Span::styled("▀", hb(BG, GREEN_DK)),
    ]);

    // Line 7: tiny feet
    //    ▫ ▫
    let line7 = Line::from(vec![
        Span::raw("   "),
        Span::styled("█", fg(FEET)),
        Span::raw("   "),
        Span::styled("█", fg(FEET)),
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
