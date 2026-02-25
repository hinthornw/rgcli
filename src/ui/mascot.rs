use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

// Parrot color palette
const GREEN_BRIGHT: Color = Color::Rgb(34, 180, 34);
const GREEN_DARK: Color = Color::Rgb(20, 120, 20);
const BELLY: Color = Color::Rgb(180, 220, 40);
const BEAK: Color = Color::Rgb(255, 165, 0);
const EYE_WHITE: Color = Color::Rgb(240, 240, 240);
const EYE_PUPIL: Color = Color::Rgb(20, 20, 20);
const CREST: Color = Color::Rgb(220, 50, 50);
const WING_BLUE: Color = Color::Rgb(40, 100, 200);
const FEET: Color = Color::Rgb(100, 100, 100);
const BG: Color = Color::Reset;

/// Half-block helper: top color = bg, bottom color = fg when using '▄'
fn hb(top: Color, bottom: Color) -> Style {
    Style::default().fg(bottom).bg(top)
}

/// Solid block of one color
fn solid(c: Color) -> Style {
    Style::default().fg(c).bg(c)
}

/// Foreground-only style
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
            ParrotState::Runs => 6,
            _ => 10,
        };
        if self.tick % speed == 0 {
            self.frame += 1;
        }
    }

    /// Returns the parrot as colored Lines, ready to render
    pub fn render(&self) -> Vec<Line<'static>> {
        match self.state {
            ParrotState::Idle => self.render_idle(),
            ParrotState::Typing => self.render_typing(),
            ParrotState::Thinking => self.render_thinking(),
            ParrotState::Threads => self.render_threads(),
            ParrotState::Assistants => self.render_assistants(),
            ParrotState::Runs => self.render_runs(),
            ParrotState::Store => self.render_store(),
            ParrotState::Crons => self.render_crons(),
            ParrotState::Logs => self.render_logs(),
        }
    }

    fn render_idle(&self) -> Vec<Line<'static>> {
        let blink = self.frame % 12 == 0;
        let look_down = self.frame % 12 == 6;
        if blink {
            parrot_base(EyeStyle::Blink, false)
        } else if look_down {
            parrot_base(EyeStyle::Down, false)
        } else {
            parrot_base(EyeStyle::Forward, false)
        }
    }

    fn render_typing(&self) -> Vec<Line<'static>> {
        let look_right = self.frame % 2 == 0;
        if look_right {
            parrot_base(EyeStyle::DownRight, false)
        } else {
            parrot_base(EyeStyle::DownLeft, false)
        }
    }

    fn render_thinking(&self) -> Vec<Line<'static>> {
        let dots = match self.frame % 4 {
            0 => " .",
            1 => " ..",
            2 => " ...",
            _ => "",
        };
        let tilted = self.frame % 4 != 3;
        let mut lines = parrot_base(if tilted { EyeStyle::Up } else { EyeStyle::Forward }, tilted);
        // Add thinking dots to the right of the top line
        if !dots.is_empty() {
            if let Some(line) = lines.get_mut(2) {
                line.spans.push(Span::styled(
                    dots.to_string(),
                    Style::default().fg(Color::Rgb(200, 200, 200)),
                ));
            }
        }
        lines
    }

    fn render_threads(&self) -> Vec<Line<'static>> {
        // Parrot with a tiny scroll
        let mut lines = parrot_base(EyeStyle::Forward, false);
        if let Some(line) = lines.get_mut(5) {
            line.spans.push(Span::styled(" 📜", fg(Color::White)));
        }
        lines
    }

    fn render_assistants(&self) -> Vec<Line<'static>> {
        // Parrot with glasses
        parrot_with_glasses()
    }

    fn render_runs(&self) -> Vec<Line<'static>> {
        // Flapping wings animation
        let flap = self.frame % 2 == 0;
        parrot_flapping(flap)
    }

    fn render_store(&self) -> Vec<Line<'static>> {
        let mut lines = parrot_base(EyeStyle::Up, false);
        // Thought bubble above
        if let Some(line) = lines.first_mut() {
            line.spans
                .insert(0, Span::styled("  o ", fg(Color::Rgb(180, 180, 180))));
        }
        lines.insert(
            0,
            Line::from(Span::styled(
                "     ( ? )",
                fg(Color::Rgb(180, 180, 180)),
            )),
        );
        lines
    }

    fn render_crons(&self) -> Vec<Line<'static>> {
        let mut lines = parrot_base(EyeStyle::Forward, false);
        if let Some(line) = lines.get_mut(1) {
            line.spans
                .push(Span::styled(" \u{23f0}", fg(Color::White)));
        }
        lines
    }

    fn render_logs(&self) -> Vec<Line<'static>> {
        let mut lines = parrot_base(EyeStyle::Forward, false);
        if let Some(line) = lines.get_mut(3) {
            line.spans
                .push(Span::styled(" \u{1f50d}", fg(Color::White)));
        }
        lines
    }
}

#[derive(Clone, Copy)]
enum EyeStyle {
    Forward,
    Blink,
    Down,
    DownLeft,
    DownRight,
    Up,
}

/// Main parrot sprite (~10 lines tall, ~14 chars wide)
/// Uses half-block trick: '▄' shows bg on top, fg on bottom
fn parrot_base(eyes: EyeStyle, _tilted: bool) -> Vec<Line<'static>> {
    // Line 0: crest tips
    //       ▄▄▓▓▄▄
    let line0 = Line::from(vec![
        Span::styled("      ", Style::default()),
        Span::styled("▄▄", hb(BG, CREST)),
        Span::styled("▄▄", hb(BG, CREST)),
    ]);

    // Line 1: crest + head top
    //     ▄▓████▓▄
    let line1 = Line::from(vec![
        Span::styled("    ", Style::default()),
        Span::styled("▄", hb(BG, CREST)),
        Span::styled("▓", fg(CREST)),
        Span::styled("████", solid(GREEN_BRIGHT)),
        Span::styled("▓", fg(CREST)),
        Span::styled("▄", hb(BG, CREST)),
    ]);

    // Line 2: eyes row
    let (left_eye, right_eye) = match eyes {
        EyeStyle::Forward => ("◉", "◉"),
        EyeStyle::Blink => ("─", "─"),
        EyeStyle::Down => ("◒", "◒"),
        EyeStyle::DownLeft => ("◐", "◑"),
        EyeStyle::DownRight => ("◑", "◐"),
        EyeStyle::Up => ("◓", "◓"),
    };
    let line2 = Line::from(vec![
        Span::styled("   ", Style::default()),
        Span::styled("▐", fg(GREEN_DARK)),
        Span::styled("█", solid(GREEN_BRIGHT)),
        Span::styled(left_eye, Style::default().fg(EYE_PUPIL).bg(EYE_WHITE)),
        Span::styled("██", solid(GREEN_BRIGHT)),
        Span::styled(right_eye, Style::default().fg(EYE_PUPIL).bg(EYE_WHITE)),
        Span::styled("█", solid(GREEN_BRIGHT)),
        Span::styled("▌", fg(GREEN_DARK)),
    ]);

    // Line 3: cheeks
    let line3 = Line::from(vec![
        Span::styled("   ", Style::default()),
        Span::styled("▐", fg(GREEN_DARK)),
        Span::styled("██", solid(GREEN_BRIGHT)),
        Span::styled("▄▄", hb(GREEN_BRIGHT, BEAK)),
        Span::styled("██", solid(GREEN_BRIGHT)),
        Span::styled("▌", fg(GREEN_DARK)),
    ]);

    // Line 4: beak
    let line4 = Line::from(vec![
        Span::styled("    ", Style::default()),
        Span::styled("█", solid(GREEN_BRIGHT)),
        Span::styled("▄", hb(GREEN_BRIGHT, BEAK)),
        Span::styled("▀▀", Style::default().fg(BEAK).bg(BEAK)),
        Span::styled("▄", hb(GREEN_BRIGHT, BEAK)),
        Span::styled("█", solid(GREEN_BRIGHT)),
    ]);

    // Line 5: upper body
    let line5 = Line::from(vec![
        Span::styled("   ", Style::default()),
        Span::styled("▐", fg(WING_BLUE)),
        Span::styled("██████", solid(GREEN_BRIGHT)),
        Span::styled("▌", fg(WING_BLUE)),
    ]);

    // Line 6: belly
    let line6 = Line::from(vec![
        Span::styled("   ", Style::default()),
        Span::styled("▐", fg(WING_BLUE)),
        Span::styled("█", solid(GREEN_DARK)),
        Span::styled("▓▓▓▓", Style::default().fg(BELLY).bg(GREEN_DARK)),
        Span::styled("█", solid(GREEN_DARK)),
        Span::styled("▌", fg(WING_BLUE)),
    ]);

    // Line 7: lower body
    let line7 = Line::from(vec![
        Span::styled("   ", Style::default()),
        Span::styled("▐", fg(WING_BLUE)),
        Span::styled("█", solid(GREEN_DARK)),
        Span::styled("░░░░", Style::default().fg(BELLY).bg(GREEN_DARK)),
        Span::styled("█", solid(GREEN_DARK)),
        Span::styled("▌", fg(WING_BLUE)),
    ]);

    // Line 8: tail base
    let line8 = Line::from(vec![
        Span::styled("    ", Style::default()),
        Span::styled("▀█", hb(BG, GREEN_DARK)),
        Span::styled("▄▄", hb(GREEN_DARK, FEET)),
        Span::styled("█▀", hb(BG, GREEN_DARK)),
    ]);

    // Line 9: feet
    let line9 = Line::from(vec![
        Span::styled("     ", Style::default()),
        Span::styled("▐▌", fg(FEET)),
        Span::styled(" ", Style::default()),
        Span::styled("▐▌", fg(FEET)),
    ]);

    vec![line0, line1, line2, line3, line4, line5, line6, line7, line8, line9]
}

/// Parrot wearing tiny glasses (assistants screen)
fn parrot_with_glasses() -> Vec<Line<'static>> {
    let line0 = Line::from(vec![
        Span::styled("      ", Style::default()),
        Span::styled("▄▄", hb(BG, CREST)),
        Span::styled("▄▄", hb(BG, CREST)),
    ]);

    let line1 = Line::from(vec![
        Span::styled("    ", Style::default()),
        Span::styled("▄", hb(BG, CREST)),
        Span::styled("▓", fg(CREST)),
        Span::styled("████", solid(GREEN_BRIGHT)),
        Span::styled("▓", fg(CREST)),
        Span::styled("▄", hb(BG, CREST)),
    ]);

    // Glasses: [◉]─[◉]
    let line2 = Line::from(vec![
        Span::styled("   ", Style::default()),
        Span::styled("▐", fg(GREEN_DARK)),
        Span::styled("[", Style::default().fg(Color::Rgb(180, 180, 180)).bg(GREEN_BRIGHT)),
        Span::styled("◉", Style::default().fg(EYE_PUPIL).bg(EYE_WHITE)),
        Span::styled("]", Style::default().fg(Color::Rgb(180, 180, 180)).bg(GREEN_BRIGHT)),
        Span::styled("─", Style::default().fg(Color::Rgb(180, 180, 180)).bg(GREEN_BRIGHT)),
        Span::styled("[", Style::default().fg(Color::Rgb(180, 180, 180)).bg(GREEN_BRIGHT)),
        Span::styled("◉", Style::default().fg(EYE_PUPIL).bg(EYE_WHITE)),
        Span::styled("]", Style::default().fg(Color::Rgb(180, 180, 180)).bg(GREEN_BRIGHT)),
        Span::styled("▌", fg(GREEN_DARK)),
    ]);

    let line3 = Line::from(vec![
        Span::styled("   ", Style::default()),
        Span::styled("▐", fg(GREEN_DARK)),
        Span::styled("██", solid(GREEN_BRIGHT)),
        Span::styled("▄▄", hb(GREEN_BRIGHT, BEAK)),
        Span::styled("██", solid(GREEN_BRIGHT)),
        Span::styled("▌", fg(GREEN_DARK)),
    ]);

    let line4 = Line::from(vec![
        Span::styled("    ", Style::default()),
        Span::styled("█", solid(GREEN_BRIGHT)),
        Span::styled("▄", hb(GREEN_BRIGHT, BEAK)),
        Span::styled("▀▀", Style::default().fg(BEAK).bg(BEAK)),
        Span::styled("▄", hb(GREEN_BRIGHT, BEAK)),
        Span::styled("█", solid(GREEN_BRIGHT)),
    ]);

    let line5 = Line::from(vec![
        Span::styled("   ", Style::default()),
        Span::styled("▐", fg(WING_BLUE)),
        Span::styled("██████", solid(GREEN_BRIGHT)),
        Span::styled("▌", fg(WING_BLUE)),
    ]);

    let line6 = Line::from(vec![
        Span::styled("   ", Style::default()),
        Span::styled("▐", fg(WING_BLUE)),
        Span::styled("█", solid(GREEN_DARK)),
        Span::styled("▓▓▓▓", Style::default().fg(BELLY).bg(GREEN_DARK)),
        Span::styled("█", solid(GREEN_DARK)),
        Span::styled("▌", fg(WING_BLUE)),
    ]);

    let line7 = Line::from(vec![
        Span::styled("   ", Style::default()),
        Span::styled("▐", fg(WING_BLUE)),
        Span::styled("█", solid(GREEN_DARK)),
        Span::styled("░░░░", Style::default().fg(BELLY).bg(GREEN_DARK)),
        Span::styled("█", solid(GREEN_DARK)),
        Span::styled("▌", fg(WING_BLUE)),
    ]);

    let line8 = Line::from(vec![
        Span::styled("    ", Style::default()),
        Span::styled("▀█", hb(BG, GREEN_DARK)),
        Span::styled("▄▄", hb(GREEN_DARK, FEET)),
        Span::styled("█▀", hb(BG, GREEN_DARK)),
    ]);

    let line9 = Line::from(vec![
        Span::styled("     ", Style::default()),
        Span::styled("▐▌", fg(FEET)),
        Span::styled(" ", Style::default()),
        Span::styled("▐▌", fg(FEET)),
    ]);

    vec![line0, line1, line2, line3, line4, line5, line6, line7, line8, line9]
}

/// Parrot with flapping wings (runs screen)
fn parrot_flapping(wings_up: bool) -> Vec<Line<'static>> {
    let line0 = Line::from(vec![
        Span::styled("      ", Style::default()),
        Span::styled("▄▄", hb(BG, CREST)),
        Span::styled("▄▄", hb(BG, CREST)),
    ]);

    let line1 = Line::from(vec![
        Span::styled("    ", Style::default()),
        Span::styled("▄", hb(BG, CREST)),
        Span::styled("▓", fg(CREST)),
        Span::styled("████", solid(GREEN_BRIGHT)),
        Span::styled("▓", fg(CREST)),
        Span::styled("▄", hb(BG, CREST)),
    ]);

    let line2 = Line::from(vec![
        Span::styled("   ", Style::default()),
        Span::styled("▐", fg(GREEN_DARK)),
        Span::styled("█", solid(GREEN_BRIGHT)),
        Span::styled("◉", Style::default().fg(EYE_PUPIL).bg(EYE_WHITE)),
        Span::styled("██", solid(GREEN_BRIGHT)),
        Span::styled("◉", Style::default().fg(EYE_PUPIL).bg(EYE_WHITE)),
        Span::styled("█", solid(GREEN_BRIGHT)),
        Span::styled("▌", fg(GREEN_DARK)),
    ]);

    let line3 = Line::from(vec![
        Span::styled("   ", Style::default()),
        Span::styled("▐", fg(GREEN_DARK)),
        Span::styled("██", solid(GREEN_BRIGHT)),
        Span::styled("▄▄", hb(GREEN_BRIGHT, BEAK)),
        Span::styled("██", solid(GREEN_BRIGHT)),
        Span::styled("▌", fg(GREEN_DARK)),
    ]);

    let line4 = Line::from(vec![
        Span::styled("    ", Style::default()),
        Span::styled("█", solid(GREEN_BRIGHT)),
        Span::styled("▄", hb(GREEN_BRIGHT, BEAK)),
        Span::styled("▀▀", Style::default().fg(BEAK).bg(BEAK)),
        Span::styled("▄", hb(GREEN_BRIGHT, BEAK)),
        Span::styled("█", solid(GREEN_BRIGHT)),
    ]);

    // Wings up or down
    let (wing_l, wing_r) = if wings_up {
        ("╱", "╲")
    } else {
        ("╲", "╱")
    };

    let line5 = Line::from(vec![
        Span::styled("  ", Style::default()),
        Span::styled(wing_l, fg(WING_BLUE)),
        Span::styled("▐", fg(WING_BLUE)),
        Span::styled("██████", solid(GREEN_BRIGHT)),
        Span::styled("▌", fg(WING_BLUE)),
        Span::styled(wing_r, fg(WING_BLUE)),
    ]);

    let line6 = Line::from(vec![
        Span::styled("   ", Style::default()),
        Span::styled("▐", fg(WING_BLUE)),
        Span::styled("█", solid(GREEN_DARK)),
        Span::styled("▓▓▓▓", Style::default().fg(BELLY).bg(GREEN_DARK)),
        Span::styled("█", solid(GREEN_DARK)),
        Span::styled("▌", fg(WING_BLUE)),
    ]);

    let line7 = Line::from(vec![
        Span::styled("   ", Style::default()),
        Span::styled("▐", fg(WING_BLUE)),
        Span::styled("█", solid(GREEN_DARK)),
        Span::styled("░░░░", Style::default().fg(BELLY).bg(GREEN_DARK)),
        Span::styled("█", solid(GREEN_DARK)),
        Span::styled("▌", fg(WING_BLUE)),
    ]);

    let line8 = Line::from(vec![
        Span::styled("    ", Style::default()),
        Span::styled("▀█", hb(BG, GREEN_DARK)),
        Span::styled("▄▄", hb(GREEN_DARK, FEET)),
        Span::styled("█▀", hb(BG, GREEN_DARK)),
    ]);

    let line9 = Line::from(vec![
        Span::styled("     ", Style::default()),
        Span::styled("▐▌", fg(FEET)),
        Span::styled(" ", Style::default()),
        Span::styled("▐▌", fg(FEET)),
    ]);

    vec![line0, line1, line2, line3, line4, line5, line6, line7, line8, line9]
}

/// Render the parrot for the welcome/logo area, with info text beside it
pub fn logo_with_parrot(
    version: &str,
    endpoint: &str,
    config_path: &str,
    context_info: &str,
    deploy_info: Option<&str>,
) -> Vec<Line<'static>> {
    let parrot_lines = parrot_base(EyeStyle::Forward, false);

    let title_style = Style::default()
        .fg(Color::Cyan)
        .add_modifier(ratatui::style::Modifier::BOLD);
    let info_style = Style::default()
        .fg(Color::DarkGray)
        .add_modifier(ratatui::style::Modifier::ITALIC);

    let info_texts: Vec<Option<Vec<Span<'static>>>> = vec![
        None, // line 0: no text
        None, // line 1: no text
        Some(vec![
            Span::styled("   ailsd", title_style),
            Span::raw(" "),
            Span::styled(version.to_string(), info_style),
        ]),
        Some(vec![
            Span::styled(format!("   {endpoint}"), info_style),
        ]),
        Some(vec![
            Span::styled(format!("   {context_info}"), info_style),
        ]),
        Some(vec![
            Span::styled(format!("   {config_path}"), info_style),
        ]),
        deploy_info.map(|info| vec![
            Span::styled(format!("   {info}"), info_style),
        ]),
        None, // line 7
        None, // line 8
        None, // line 9
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
