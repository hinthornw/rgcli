use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

// Kawaii parrot palette
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
    Deployments,
}

pub struct Parrot {
    state: ParrotState,
    frame: usize,
    tick: usize,
    rng: u32, // simple xorshift rng state
}

impl Parrot {
    pub fn new() -> Self {
        Self {
            state: ParrotState::Idle,
            frame: 0,
            tick: 0,
            rng: 0xDEAD_BEEF,
        }
    }

    pub fn set_state(&mut self, state: ParrotState) {
        if std::mem::discriminant(&self.state) != std::mem::discriminant(&state) {
            self.state = state;
            self.frame = 0;
            self.tick = 0;
        }
    }

    /// Simple xorshift PRNG — returns a pseudo-random u32.
    fn rand(&mut self) -> u32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        self.rng
    }

    pub fn tick(&mut self) {
        self.tick += 1;
        let base_speed = match self.state {
            ParrotState::Thinking => 4,
            ParrotState::Idle => 6, // ~480ms per frame — visible dance
            ParrotState::Runs => 5,
            _ => 10,
        };
        if self.tick % base_speed == 0 {
            // Occasionally stutter: 20% chance to hold the current frame an extra beat
            if self.state == ParrotState::Idle && self.rand() % 5 == 0 {
                return; // skip this frame advance
            }
            self.frame += 1;
        }
    }

    pub fn render(&mut self) -> Vec<Line<'static>> {
        match self.state {
            ParrotState::Idle => self.render_idle(),
            ParrotState::Typing => self.render_typing(),
            ParrotState::Thinking => self.render_thinking(),
            ParrotState::Threads => kawaii_parrot(Pose::default()),
            ParrotState::Assistants => kawaii_parrot(Pose::default()),
            ParrotState::Runs => self.render_runs(),
            ParrotState::Store => self.render_thinking(),
            ParrotState::Crons => kawaii_parrot(Pose::default()),
            ParrotState::Logs => kawaii_parrot(Pose::default()),
            ParrotState::Deployments => kawaii_parrot(Pose::default()),
        }
    }

    fn render_idle(&mut self) -> Vec<Line<'static>> {
        // Occasionally throw in a surprise pose (~10% of frames)
        if self.rand() % 10 == 0 {
            let surprise = match self.rand() % 4 {
                0 => Pose {
                    eyes: Eyes::Sparkle,
                    bounce: true,
                    wing_left: WingPos::Up,
                    wing_right: WingPos::Up,
                    ..Pose::default()
                },
                1 => Pose {
                    eyes: Eyes::Happy,
                    tilt: -1,
                    wing_left: WingPos::Up,
                    foot_right: false,
                    ..Pose::default()
                },
                2 => Pose {
                    eyes: Eyes::Happy,
                    tilt: 1,
                    wing_right: WingPos::Up,
                    foot_left: false,
                    ..Pose::default()
                },
                _ => Pose {
                    eyes: Eyes::Normal,
                    bounce: true,
                    wing_left: WingPos::Up,
                    wing_right: WingPos::Up,
                    foot_left: false,
                    foot_right: false,
                    ..Pose::default()
                },
            };
            return kawaii_parrot(surprise);
        }

        // 8-frame kawaii dance cycle
        let pose = match self.frame % 8 {
            0 => Pose {
                eyes: Eyes::Normal,
                ..Pose::default()
            },
            1 => Pose {
                eyes: Eyes::Happy,
                bounce: true,
                wing_left: WingPos::Up,
                foot_right: false,
                ..Pose::default()
            },
            2 => Pose {
                eyes: Eyes::LookLeft,
                tilt: -1,
                wing_left: WingPos::Up,
                wing_right: WingPos::Down,
                ..Pose::default()
            },
            3 => Pose {
                eyes: Eyes::Happy,
                bounce: true,
                wing_left: WingPos::Up,
                wing_right: WingPos::Up,
                ..Pose::default()
            },
            4 => Pose {
                eyes: Eyes::Sparkle,
                ..Pose::default()
            },
            5 => Pose {
                eyes: Eyes::Happy,
                bounce: true,
                wing_right: WingPos::Up,
                foot_left: false,
                ..Pose::default()
            },
            6 => Pose {
                eyes: Eyes::LookRight,
                tilt: 1,
                wing_left: WingPos::Down,
                wing_right: WingPos::Up,
                ..Pose::default()
            },
            7 => Pose {
                eyes: Eyes::Happy,
                bounce: true,
                wing_left: WingPos::Up,
                wing_right: WingPos::Up,
                ..Pose::default()
            },
            _ => Pose::default(),
        };
        kawaii_parrot(pose)
    }

    fn render_typing(&self) -> Vec<Line<'static>> {
        let pose = if self.frame % 2 == 0 {
            Pose {
                eyes: Eyes::LookRight,
                ..Pose::default()
            }
        } else {
            Pose {
                eyes: Eyes::LookLeft,
                ..Pose::default()
            }
        };
        kawaii_parrot(pose)
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
        let pose = Pose {
            eyes,
            bounce: self.frame % 4 < 2,
            wing_left: if self.frame % 2 == 0 {
                WingPos::Up
            } else {
                WingPos::Down
            },
            wing_right: if self.frame % 2 == 1 {
                WingPos::Up
            } else {
                WingPos::Down
            },
            ..Pose::default()
        };
        let mut lines = kawaii_parrot(pose);
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
        let pose = if self.frame % 2 == 0 {
            Pose {
                eyes: Eyes::Sparkle,
                ..Pose::default()
            }
        } else {
            Pose {
                eyes: Eyes::Happy,
                bounce: true,
                ..Pose::default()
            }
        };
        kawaii_parrot(pose)
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

#[derive(Clone, Copy, PartialEq)]
enum WingPos {
    Down,
    Up,
}

struct Pose {
    eyes: Eyes,
    bounce: bool,    // shift body up (add padding at bottom)
    tilt: i8,        // -1 = lean left, 0 = center, 1 = lean right
    wing_left: WingPos,
    wing_right: WingPos,
    foot_left: bool,  // show left foot
    foot_right: bool, // show right foot
}

impl Default for Pose {
    fn default() -> Self {
        Self {
            eyes: Eyes::Normal,
            bounce: false,
            tilt: 0,
            wing_left: WingPos::Down,
            wing_right: WingPos::Down,
            foot_left: true,
            foot_right: true,
        }
    }
}

fn kawaii_parrot(pose: Pose) -> Vec<Line<'static>> {
    let g = on(GREEN_LT, GREEN_LT);

    // Tilt offset — shift the leading spaces
    let pad = |base: usize| -> String {
        let adjusted = (base as i8 + pose.tilt).max(0) as usize;
        " ".repeat(adjusted)
    };

    // Line 0: crest
    let line0 = Line::from(vec![
        Span::raw(pad(4)),
        Span::styled("▄", hb(BG, CREST)),
        Span::styled("█", on(CREST, CREST)),
        Span::styled("▄", hb(BG, CREST)),
    ]);

    // Line 1: top of head
    let line1 = Line::from(vec![
        Span::raw(pad(2)),
        Span::styled("▄", hb(BG, GREEN_LT)),
        Span::styled("▄", hb(BG, GREEN_LT)),
        Span::styled("▄▄▄▄▄", hb(CREST, GREEN_LT)),
        Span::styled("▄", hb(BG, GREEN_LT)),
        Span::styled("▄", hb(BG, GREEN_LT)),
    ]);

    // Line 2: eyes
    let (le, re) = match pose.eyes {
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
    let line2 = Line::from(vec![
        Span::raw(pad(1)),
        Span::styled("▐", fg(GREEN_DK)),
        Span::styled(" ", g),
        le,
        Span::styled("   ", g),
        re,
        Span::styled(" ", g),
        Span::styled("▌", fg(GREEN_DK)),
    ]);

    // Line 3: blush + beak
    let line3 = Line::from(vec![
        Span::raw(pad(1)),
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

    // Line 4: body transition with wings
    let lw = if pose.wing_left == WingPos::Up {
        Span::styled("▟", fg(WING))
    } else {
        Span::styled("▗", fg(WING))
    };
    let rw = if pose.wing_right == WingPos::Up {
        Span::styled("▙", fg(WING))
    } else {
        Span::styled("▖", fg(WING))
    };
    let line4 = Line::from(vec![
        lw,
        Span::styled("▐", fg(GREEN_DK)),
        Span::styled("▄", hb(GREEN_LT, BELLY)),
        Span::styled("▄▄▄▄▄", hb(GREEN_LT, BELLY)),
        Span::styled("▄", hb(GREEN_LT, BELLY)),
        Span::styled("▌", fg(GREEN_DK)),
        rw,
    ]);

    // Line 5: belly
    let line5 = Line::from(vec![
        Span::raw(pad(1)),
        Span::styled("▐", fg(GREEN_DK)),
        Span::styled("       ", on(BELLY, BELLY)),
        Span::styled("▌", fg(GREEN_DK)),
    ]);

    // Line 6: bottom curve
    let line6 = Line::from(vec![
        Span::raw(pad(2)),
        Span::styled("▀", hb(BG, GREEN_DK)),
        Span::styled("▀▀▀▀▀", hb(BG, BELLY)),
        Span::styled("▀", hb(BG, GREEN_DK)),
    ]);

    // Line 7: feet — can hide one for a tap effect
    let line7 = if pose.bounce {
        // Bounced up — no feet visible (floating!)
        Line::from(Span::raw(""))
    } else {
        let lf = if pose.foot_left {
            Span::styled("█", fg(FEET))
        } else {
            Span::raw(" ")
        };
        let rf = if pose.foot_right {
            Span::styled("█", fg(FEET))
        } else {
            Span::raw(" ")
        };
        Line::from(vec![
            Span::raw(pad(3)),
            lf,
            Span::raw("   "),
            rf,
        ])
    };

    // If bouncing, add empty line at top to keep 8 lines total
    if pose.bounce {
        vec![
            Line::from(Span::raw("")),
            line0,
            line1,
            line2,
            line3,
            line4,
            line5,
            line6,
        ]
    } else {
        vec![line0, line1, line2, line3, line4, line5, line6, line7]
    }
}

/// Render the parrot for the welcome/logo area, with info text beside it
pub fn logo_with_parrot(
    version: &str,
    endpoint: &str,
    config_path: &str,
    context_info: &str,
    deploy_info: Option<&str>,
) -> Vec<Line<'static>> {
    let parrot_lines = kawaii_parrot(Pose::default());

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
