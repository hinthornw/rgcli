use std::time::{Duration, Instant};

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
    Curious,      // typing long input (>30 chars)
    SlashCommand, // typing a / command
    Thinking,
    FeedbackHappy,
    FeedbackSad,
    Error,
    Sleeping,
    Interrupted,
    Threads,
    Assistants,
    Runs,
    Store,
    Crons,
    Deployments,
}

pub struct Parrot {
    state: ParrotState,
    frame: usize,
    tick: usize,
    rng: u32,
    state_expires: Option<Instant>,
    berserk: bool, // 1/10 chance on error — parrot freaks out
    pub pos_x: u16,       // horizontal position within box
    pace_dir: i8,          // 1 = moving right, -1 = moving left
    pub box_width: u16,    // set by renderer so parrot knows bounds
}

impl Parrot {
    pub fn new() -> Self {
        Self {
            state: ParrotState::Idle,
            frame: 0,
            tick: 0,
            rng: 0xDEAD_BEEF,
            state_expires: None,
            berserk: false,
            pos_x: 6,
            pace_dir: 1,
            box_width: 20,
        }
    }

    pub fn set_state(&mut self, state: ParrotState) {
        if std::mem::discriminant(&self.state) != std::mem::discriminant(&state) {
            self.state = state;
            self.frame = 0;
            self.tick = 0;
            self.state_expires = None;
        }
    }

    pub fn set_timed_state(&mut self, state: ParrotState, duration: Duration) {
        if state == ParrotState::Error {
            // 1 in 10 chance to go berserk
            let mut rng = self.rng;
            rng ^= rng << 13;
            rng ^= rng >> 17;
            rng ^= rng << 5;
            self.rng = rng;
            self.berserk = rng % 10 == 0;
            if self.berserk {
                // Berserk lasts longer so you can enjoy the meltdown
                self.state_expires = Some(Instant::now() + Duration::from_secs(6));
                self.state = state;
                self.frame = 0;
                self.tick = 0;
                return;
            }
        }
        self.berserk = false;
        self.state = state;
        self.state_expires = Some(Instant::now() + duration);
        self.frame = 0;
        self.tick = 0;
    }

    pub fn check_expiry(&mut self) {
        if let Some(expires) = self.state_expires {
            if Instant::now() >= expires {
                self.state_expires = None;
                self.berserk = false;
                self.state = ParrotState::Idle;
                self.frame = 0;
                self.tick = 0;
            }
        }
    }

    pub fn has_timed_state(&self) -> bool {
        self.state_expires.is_some()
    }

    pub fn current_state(&self) -> &ParrotState {
        &self.state
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
            ParrotState::Idle => 12, // calm, slow transitions
            ParrotState::Runs => 5,
            ParrotState::Sleeping => 12, // slow Z animation
            ParrotState::FeedbackHappy | ParrotState::FeedbackSad => 3, // fast celebration
            ParrotState::Error => if self.berserk { 1 } else { 3 },
            _ => 10,
        };
        if self.tick % base_speed == 0 {
            // Occasionally stutter: 20% chance to hold the current frame an extra beat
            if self.state == ParrotState::Idle && self.rand() % 5 == 0 {
                return; // skip this frame advance
            }
            self.frame += 1;
        }

        // Pacing: move horizontally every ~6 ticks during idle
        // Pace during most of the cycle (frames 4-13), only rest at the ends
        if self.state == ParrotState::Idle && self.tick % 6 == 0 {
            let sprite_width: u16 = 11;
            let max_x = self.box_width.saturating_sub(sprite_width);
            if max_x > 0 {
                let idle_frame = self.frame % 16;
                if (4..=13).contains(&idle_frame) {
                    // Pacing phase: move in current direction
                    let new_x = self.pos_x as i16 + self.pace_dir as i16;
                    if new_x <= 0 {
                        self.pos_x = 0;
                        self.pace_dir = 1;
                    } else if new_x >= max_x as i16 {
                        self.pos_x = max_x;
                        self.pace_dir = -1;
                    } else {
                        self.pos_x = new_x as u16;
                    }
                }
            }
        }

        // Berserk: jitter position randomly
        if self.berserk && self.state == ParrotState::Error {
            let sprite_width: u16 = 11;
            let max_x = self.box_width.saturating_sub(sprite_width);
            if max_x > 0 {
                self.pos_x = (self.rand() as u16) % (max_x + 1);
            }
        }
    }

    pub fn render(&mut self) -> Vec<Line<'static>> {
        match self.state {
            ParrotState::Idle => self.render_idle(),
            ParrotState::Typing | ParrotState::Curious | ParrotState::SlashCommand => {
                self.render_typing()
            }
            ParrotState::Thinking => self.render_thinking(),
            ParrotState::FeedbackHappy => kawaii_parrot(Pose {
                eyes: Eyes::Happy,
                bounce: self.frame % 2 == 0,
                wing_left: WingPos::Up,
                wing_right: WingPos::Up,
                ..Pose::default()
            }),
            ParrotState::FeedbackSad => kawaii_parrot(Pose {
                eyes: Eyes::Normal,
                tilt: if self.frame % 2 == 0 { -1 } else { 0 },
                ..Pose::default()
            }),
            ParrotState::Error => self.render_error(),
            ParrotState::Sleeping => kawaii_parrot(Pose {
                eyes: Eyes::Happy, // closed eyes
                ..Pose::default()
            }),
            ParrotState::Interrupted => kawaii_parrot(Pose {
                eyes: Eyes::Normal,
                ..Pose::default()
            }),
            ParrotState::Threads => kawaii_parrot(Pose::default()),
            ParrotState::Assistants => kawaii_parrot(Pose::default()),
            ParrotState::Runs => self.render_runs(),
            ParrotState::Store => self.render_thinking(),
            ParrotState::Crons => kawaii_parrot(Pose::default()),
            ParrotState::Deployments => kawaii_parrot(Pose::default()),
        }
    }

    /// Compact 1-line face for the status bar.
    pub fn mini_face(&mut self) -> Span<'static> {
        let face_style = Style::default().fg(GREEN_LT);
        let happy_style = Style::default().fg(Color::Rgb(100, 220, 120));
        let sad_style = Style::default().fg(Color::Rgb(180, 180, 100));
        let error_style = Style::default().fg(Color::Rgb(255, 100, 100));
        let sleep_style = Style::default().fg(Color::Rgb(120, 120, 160));
        let sparkle_style = Style::default().fg(Color::Rgb(255, 220, 100));

        match &self.state {
            ParrotState::Idle => {
                let faces = ["(◕‿◕)", "(◡‿◡)", "(✦‿✦)"];
                Span::styled(faces[self.frame % faces.len()].to_string(), face_style)
            }
            ParrotState::Typing => Span::styled("(◕_◕)", face_style),
            ParrotState::Curious => {
                let faces = ["(◕◕)…", "(◕◕)‥"];
                Span::styled(faces[self.frame % faces.len()].to_string(), face_style)
            }
            ParrotState::SlashCommand => Span::styled("(✦‿✦)/", sparkle_style),
            ParrotState::Thinking => {
                let dots = match self.frame % 4 {
                    0 => "(✦‿✦).",
                    1 => "(✦‿✦)..",
                    2 => "(✦‿✦)...",
                    _ => "(✦‿✦)",
                };
                Span::styled(dots.to_string(), sparkle_style)
            }
            ParrotState::FeedbackHappy => {
                let s = if self.frame % 2 == 0 {
                    "(◕‿◕)♡"
                } else {
                    "(◡‿◡)♡"
                };
                Span::styled(s.to_string(), happy_style)
            }
            ParrotState::FeedbackSad => Span::styled("(◕︵◕)", sad_style),
            ParrotState::Error => {
                if self.berserk {
                    let faces = ["(╯°□°)╯︵┻━┻", "(ノಠ益ಠ)ノ彡┻━┻", "ヽ(°□°)ﾉ", "(>_<)!!!"];
                    Span::styled(faces[self.frame % faces.len()].to_string(), error_style)
                } else {
                    let s = if self.frame % 2 == 0 { "(°□°)!" } else { "(°□°) " };
                    Span::styled(s.to_string(), error_style)
                }
            }
            ParrotState::Sleeping => {
                let zs = match self.frame % 3 {
                    0 => "(-_-)ᶻ",
                    1 => "(-_-)ᶻᶻ",
                    _ => "(-_-)ᶻᶻᶻ",
                };
                Span::styled(zs.to_string(), sleep_style)
            }
            ParrotState::Interrupted => Span::styled("(◕_◕)⏸", face_style),
            _ => Span::styled("(◕‿◕)", face_style),
        }
    }

    fn render_idle(&mut self) -> Vec<Line<'static>> {
        // Calm idle: 16-frame cycle
        // Frames 0-5: sitting still (gentle blinks)
        // Frames 6-9: gentle pace (look left, look right)
        // Frames 10-13: sitting again
        // Frames 14-15: rare little dance move (~1 in 4 cycles, else sit)
        let pose = match self.frame % 16 {
            // Sitting still — occasional blink
            0..=2 => Pose {
                eyes: Eyes::Normal,
                ..Pose::default()
            },
            3 => Pose {
                eyes: Eyes::Happy, // blink
                ..Pose::default()
            },
            4..=5 => Pose {
                eyes: Eyes::Normal,
                ..Pose::default()
            },
            // Gentle pace: look around
            6 => Pose {
                eyes: Eyes::LookLeft,
                tilt: -1,
                ..Pose::default()
            },
            7 => Pose {
                eyes: Eyes::LookLeft,
                ..Pose::default()
            },
            8 => Pose {
                eyes: Eyes::LookRight,
                ..Pose::default()
            },
            9 => Pose {
                eyes: Eyes::LookRight,
                tilt: 1,
                ..Pose::default()
            },
            // Sitting again
            10..=13 => Pose {
                eyes: Eyes::Normal,
                ..Pose::default()
            },
            // Rare flourish (25% chance) or just sit
            14 => {
                if self.rand() % 4 == 0 {
                    Pose {
                        eyes: Eyes::Sparkle,
                        bounce: true,
                        wing_left: WingPos::Up,
                        wing_right: WingPos::Up,
                        ..Pose::default()
                    }
                } else {
                    Pose {
                        eyes: Eyes::Happy,
                        ..Pose::default()
                    }
                }
            }
            15 => Pose {
                eyes: Eyes::Normal,
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

    fn render_error(&mut self) -> Vec<Line<'static>> {
        if !self.berserk {
            // Normal error: shocked face, gentle bounce
            return kawaii_parrot(Pose {
                eyes: Eyes::Sparkle,
                bounce: self.frame % 2 == 0,
                ..Pose::default()
            });
        }

        // BERSERK MODE: rapid chaotic flailing
        let r = self.rand();
        let eyes = match r % 4 {
            0 => Eyes::Sparkle,
            1 => Eyes::LookLeft,
            2 => Eyes::LookRight,
            _ => Eyes::Happy,
        };
        let r2 = self.rand();
        kawaii_parrot(Pose {
            eyes,
            bounce: r2 % 2 == 0,
            tilt: match r2 % 3 {
                0 => -1,
                1 => 1,
                _ => 0,
            },
            wing_left: if r % 2 == 0 { WingPos::Up } else { WingPos::Down },
            wing_right: if r2 % 2 == 0 { WingPos::Up } else { WingPos::Down },
            foot_left: r % 3 != 0,
            foot_right: r2 % 3 != 0,
        })
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
    bounce: bool, // shift body up (add padding at bottom)
    tilt: i8,     // -1 = lean left, 0 = center, 1 = lean right
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
        Line::from(vec![Span::raw(pad(3)), lf, Span::raw("   "), rf])
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
