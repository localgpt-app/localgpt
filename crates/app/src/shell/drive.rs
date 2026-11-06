//! A hand, for checks without one: egui events on a timetable, read from
//! `LOCALGPT_APP_SHELL_DRIVE` and put into the window's input before egui
//! sees it (`eframe::App::raw_input_hook`), so they take the whole path a
//! hand's would — the panels, the world's bridge, Bevy's input.
//!
//! The script is steps separated by `;`, each `<seconds>:<what>`:
//!
//! ```text
//! 2:click 0.5 0.5        a left click, at fractions of the world panel
//! 2:tap 120 140          a left click at a point of the window
//! 3:key W 1.5            W held for 1.5 s (keys as egui names them)
//! 5:drag right 300 0     a right-button drag by points, over half a second
//! 5:swipe 80 200 40 0    a left-button drag from a point of the window
//! 5:wheel 80 600 -300    the wheel turned at a point, by points (less
//!                        than 0 brings what is below into view)
//! 6:type hello\r         text, as typed (`\r` is Enter, `\e` Escape)
//! 7:shot /tmp/a.png      a picture of the window
//! 9:quit
//! ```

use std::path::PathBuf;
use std::time::Instant;

use eframe::egui;

pub const LOCALGPT_APP_SHELL_DRIVE: &str = "LOCALGPT_APP_SHELL_DRIVE";

#[derive(Debug, Clone, PartialEq)]
enum Step {
    Click {
        x: f32,
        y: f32,
    },
    /// A click at a point of the window, in points.
    Tap {
        x: f32,
        y: f32,
    },
    Key {
        key: egui::Key,
        hold: f64,
    },
    Drag {
        button: egui::PointerButton,
        dx: f32,
        dy: f32,
    },
    /// A left-button drag from a point of the window, in points.
    Swipe {
        x: f32,
        y: f32,
        dx: f32,
        dy: f32,
    },
    /// The wheel turned at a point of the window, by points.
    Wheel {
        x: f32,
        y: f32,
        dy: f32,
    },
    Type(String),
    Shot(PathBuf),
    Quit,
}

/// An event still to come, and when.
struct Later {
    at: f64,
    event: egui::Event,
}

pub struct Drive {
    steps: Vec<(f64, Step)>,
    later: Vec<Later>,
    started: Instant,
}

impl Drive {
    pub fn from_env() -> Option<Self> {
        let script = std::env::var(LOCALGPT_APP_SHELL_DRIVE).ok()?;
        let steps = parse(&script);
        if steps.is_empty() {
            eprintln!("localgpt-app: {LOCALGPT_APP_SHELL_DRIVE} has no steps it can read");
            return None;
        }
        Some(Self {
            steps,
            later: Vec::new(),
            started: Instant::now(),
        })
    }

    /// This frame's events, for the world panel last drawn at `world`.
    pub fn inject(&mut self, ctx: &egui::Context, input: &mut egui::RawInput, world: egui::Rect) {
        let now = self.started.elapsed().as_secs_f64();
        let modifiers = egui::Modifiers::NONE;
        let (due, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut self.later)
            .into_iter()
            .partition(|l| l.at <= now);
        self.later = rest;
        input.events.extend(due.into_iter().map(|l| l.event));
        while let Some((at, _)) = self.steps.first() {
            if *at > now {
                break;
            }
            let (_, step) = self.steps.remove(0);
            tracing::debug!("drive at {now:.2} s: {step:?}");
            match step {
                Step::Click { .. } | Step::Tap { .. } => {
                    let pos = match step {
                        Step::Click { x, y } => world.lerp_inside(egui::vec2(x, y)),
                        Step::Tap { x, y } => egui::pos2(x, y),
                        _ => unreachable!("matched above"),
                    };
                    // The pointer arrives first and the press comes after, as
                    // a hand's does: a press counts on what is already under
                    // the pointer (picking's hover map), so one in the same
                    // frame as the move would land on nothing.
                    input.events.push(egui::Event::PointerMoved(pos));
                    for (after, pressed) in [(0.1, true), (0.16, false)] {
                        self.later.push(Later {
                            at: now + after,
                            event: egui::Event::PointerButton {
                                pos,
                                button: egui::PointerButton::Primary,
                                pressed,
                                modifiers,
                            },
                        });
                    }
                }
                Step::Key { key, hold } => {
                    let press = |pressed| egui::Event::Key {
                        key,
                        physical_key: Some(key),
                        pressed,
                        repeat: false,
                        modifiers,
                    };
                    input.events.push(press(true));
                    self.later.push(Later {
                        at: now + hold,
                        event: press(false),
                    });
                }
                Step::Drag { .. } | Step::Swipe { .. } => {
                    let (from, button, dx, dy) = match step {
                        Step::Drag { button, dx, dy } => (world.center(), button, dx, dy),
                        Step::Swipe { x, y, dx, dy } => {
                            (egui::pos2(x, y), egui::PointerButton::Primary, dx, dy)
                        }
                        _ => unreachable!("matched above"),
                    };
                    input.events.push(egui::Event::PointerMoved(from));
                    input.events.push(egui::Event::PointerButton {
                        pos: from,
                        button,
                        pressed: true,
                        modifiers,
                    });
                    let steps = 30;
                    for i in 1..=steps {
                        let t = i as f32 / steps as f32;
                        let at = now + 0.5 * t as f64;
                        let delta = egui::vec2(dx, dy) / steps as f32;
                        self.later.push(Later {
                            at,
                            event: egui::Event::MouseMoved(delta),
                        });
                        self.later.push(Later {
                            at,
                            event: egui::Event::PointerMoved(from + egui::vec2(dx, dy) * t),
                        });
                    }
                    self.later.push(Later {
                        at: now + 0.55,
                        event: egui::Event::PointerButton {
                            pos: from + egui::vec2(dx, dy),
                            button,
                            pressed: false,
                            modifiers,
                        },
                    });
                }
                Step::Wheel { x, y, dy } => {
                    // Over the point first: a wheel turns what is under the
                    // pointer.
                    let at = egui::pos2(x, y);
                    input.events.push(egui::Event::PointerMoved(at));
                    self.later.push(Later {
                        at: now + 0.05,
                        event: egui::Event::MouseWheel {
                            unit: egui::MouseWheelUnit::Point,
                            delta: egui::vec2(0.0, dy),
                            phase: egui::TouchPhase::Move,
                            source: egui::MouseWheelSource::Unknown,
                            modifiers,
                        },
                    });
                }
                Step::Type(text) => input.events.push(egui::Event::Text(text)),
                Step::Shot(path) => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(
                        egui::ScreenshotCallback::new(move |image| super::save_png(&path, &image)),
                    ));
                }
                Step::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            }
        }
    }
}

fn parse(script: &str) -> Vec<(f64, Step)> {
    let mut steps: Vec<(f64, Step)> = script
        .split(';')
        .filter_map(|part| {
            let (at, what) = part.trim().split_once(':')?;
            let at: f64 = at.trim().parse().ok()?;
            let mut words = what.split_whitespace();
            let step = match words.next()? {
                "click" => Step::Click {
                    x: words.next()?.parse().ok()?,
                    y: words.next()?.parse().ok()?,
                },
                "tap" => Step::Tap {
                    x: words.next()?.parse().ok()?,
                    y: words.next()?.parse().ok()?,
                },
                "key" => Step::Key {
                    key: egui::Key::from_name(words.next()?)?,
                    hold: words.next().and_then(|h| h.parse().ok()).unwrap_or(0.1),
                },
                "drag" => Step::Drag {
                    button: match words.next()? {
                        "left" => egui::PointerButton::Primary,
                        "right" => egui::PointerButton::Secondary,
                        "middle" => egui::PointerButton::Middle,
                        _ => return None,
                    },
                    dx: words.next()?.parse().ok()?,
                    dy: words.next()?.parse().ok()?,
                },
                "swipe" => Step::Swipe {
                    x: words.next()?.parse().ok()?,
                    y: words.next()?.parse().ok()?,
                    dx: words.next()?.parse().ok()?,
                    dy: words.next()?.parse().ok()?,
                },
                "wheel" => Step::Wheel {
                    x: words.next()?.parse().ok()?,
                    y: words.next()?.parse().ok()?,
                    dy: words.next()?.parse().ok()?,
                },
                "type" => {
                    let text = what.trim_start().strip_prefix("type")?.trim_start();
                    Step::Type(text.replace("\\r", "\r").replace("\\e", "\x1b"))
                }
                "shot" => Step::Shot(PathBuf::from(words.next()?)),
                "quit" => Step::Quit,
                _ => return None,
            };
            Some((at, step))
        })
        .collect();
    steps.sort_by(|a, b| a.0.total_cmp(&b.0));
    steps
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_script_reads_as_steps_in_time_order() {
        let steps = parse(
            "3:key W 1.5; 2:click 0.5 0.25; 5:drag right 300 -20; 5.5:swipe 80 200 40 0; 6:type ls\\r; 7:shot /tmp/a.png; 9:quit; x:nonsense",
        );
        assert_eq!(
            steps,
            vec![
                (2.0, Step::Click { x: 0.5, y: 0.25 }),
                (
                    3.0,
                    Step::Key {
                        key: egui::Key::W,
                        hold: 1.5
                    }
                ),
                (
                    5.0,
                    Step::Drag {
                        button: egui::PointerButton::Secondary,
                        dx: 300.0,
                        dy: -20.0
                    }
                ),
                (
                    5.5,
                    Step::Swipe {
                        x: 80.0,
                        y: 200.0,
                        dx: 40.0,
                        dy: 0.0
                    }
                ),
                (6.0, Step::Type("ls\r".into())),
                (7.0, Step::Shot(PathBuf::from("/tmp/a.png"))),
                (9.0, Step::Quit),
            ]
        );
    }
}
