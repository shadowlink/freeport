//! Reads controllers with `gilrs` (evdev on Linux — works for local pads and
//! Moonlight's virtual gamepad) and emits semantic navigation events to the
//! frontend on `gamepad://input`. Directions auto-repeat while held; action
//! buttons fire once per press.

use gilrs::{Axis, Button, Gilrs};
use serde::Serialize;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

#[derive(Clone, Serialize)]
struct GamepadInput {
    button: String,
}

const POLL: Duration = Duration::from_millis(16);
const REPEAT_DELAY: Duration = Duration::from_millis(400);
const REPEAT_RATE: Duration = Duration::from_millis(120);
const DEADZONE: f32 = 0.5;

/// Buttons that auto-repeat when held (navigation).
fn is_directional(b: &str) -> bool {
    matches!(b, "up" | "down" | "left" | "right")
}

/// Starts the controller polling thread. Safe no-op if no gamepad backend.
pub fn start(app: AppHandle) {
    std::thread::spawn(move || {
        let mut gilrs = match Gilrs::new() {
            Ok(g) => g,
            Err(_) => return,
        };
        // Per-active-input next fire time (for edge + repeat logic).
        let mut next_fire: HashMap<String, Instant> = HashMap::new();

        loop {
            // Drain events so gilrs keeps gamepad state fresh.
            while gilrs.next_event().is_some() {}

            // Collect which semantic inputs are active right now (any gamepad).
            let mut active: Vec<String> = Vec::new();
            for (_id, gp) in gilrs.gamepads() {
                let pressed = |b: Button| gp.is_pressed(b);
                if pressed(Button::DPadUp) {
                    active.push("up".into());
                }
                if pressed(Button::DPadDown) {
                    active.push("down".into());
                }
                if pressed(Button::DPadLeft) {
                    active.push("left".into());
                }
                if pressed(Button::DPadRight) {
                    active.push("right".into());
                }
                // Left stick as a d-pad.
                let lx = gp.value(Axis::LeftStickX);
                let ly = gp.value(Axis::LeftStickY);
                if ly > DEADZONE {
                    active.push("up".into());
                } else if ly < -DEADZONE {
                    active.push("down".into());
                }
                if lx < -DEADZONE {
                    active.push("left".into());
                } else if lx > DEADZONE {
                    active.push("right".into());
                }
                if pressed(Button::South) {
                    active.push("a".into());
                }
                if pressed(Button::East) {
                    active.push("b".into());
                }
                if pressed(Button::West) {
                    active.push("x".into());
                }
                if pressed(Button::North) {
                    active.push("y".into());
                }
                if pressed(Button::LeftTrigger) {
                    active.push("lb".into());
                }
                if pressed(Button::RightTrigger) {
                    active.push("rb".into());
                }
                if pressed(Button::Start) {
                    active.push("start".into());
                }
                if pressed(Button::Select) {
                    active.push("back".into());
                }
            }
            active.sort();
            active.dedup();

            let now = Instant::now();
            let emit = |app: &AppHandle, button: &str| {
                let _ = app.emit(
                    "gamepad://input",
                    GamepadInput {
                        button: button.to_string(),
                    },
                );
            };

            for b in &active {
                match next_fire.get(b) {
                    None => {
                        // Just pressed: fire now. Directions schedule a repeat;
                        // actions get a far-future time so they don't repeat.
                        emit(&app, b);
                        let next = if is_directional(b) {
                            now + REPEAT_DELAY
                        } else {
                            now + Duration::from_secs(3600)
                        };
                        next_fire.insert(b.clone(), next);
                    }
                    Some(&t) if now >= t => {
                        emit(&app, b);
                        next_fire.insert(b.clone(), now + REPEAT_RATE);
                    }
                    _ => {}
                }
            }
            // Drop inputs no longer active so they re-trigger on next press.
            next_fire.retain(|k, _| active.contains(k));

            std::thread::sleep(POLL);
        }
    });
}
