//! Reads controllers with `gilrs` (evdev on Linux — works for local pads and
//! Moonlight's virtual gamepad) and calls `on_input` with semantic button names
//! ("up","down","left","right","a","b","x","y","lb","rb","start","back").
//! Directions auto-repeat while held; action buttons fire once per press.

use gilrs::{Axis, Button, Gilrs};
use std::collections::HashMap;
use std::time::{Duration, Instant};

const POLL: Duration = Duration::from_millis(16);
const REPEAT_DELAY: Duration = Duration::from_millis(400);
const REPEAT_RATE: Duration = Duration::from_millis(120);
const DEADZONE: f32 = 0.5;

fn is_directional(b: &str) -> bool {
    matches!(b, "up" | "down" | "left" | "right")
}

/// Starts the controller polling thread. No-op if no gamepad backend is present.
pub fn start(on_input: impl Fn(&str) + Send + 'static) {
    std::thread::spawn(move || {
        let mut gilrs = match Gilrs::new() {
            Ok(g) => g,
            Err(_) => return,
        };
        let mut next_fire: HashMap<String, Instant> = HashMap::new();
        loop {
            while gilrs.next_event().is_some() {}

            let mut active: Vec<String> = Vec::new();
            for (_id, gp) in gilrs.gamepads() {
                let p = |b: Button| gp.is_pressed(b);
                if p(Button::DPadUp) { active.push("up".into()); }
                if p(Button::DPadDown) { active.push("down".into()); }
                if p(Button::DPadLeft) { active.push("left".into()); }
                if p(Button::DPadRight) { active.push("right".into()); }
                let lx = gp.value(Axis::LeftStickX);
                let ly = gp.value(Axis::LeftStickY);
                if ly > DEADZONE { active.push("up".into()); } else if ly < -DEADZONE { active.push("down".into()); }
                if lx < -DEADZONE { active.push("left".into()); } else if lx > DEADZONE { active.push("right".into()); }
                if p(Button::South) { active.push("a".into()); }
                if p(Button::East) { active.push("b".into()); }
                if p(Button::West) { active.push("x".into()); }
                if p(Button::North) { active.push("y".into()); }
                if p(Button::LeftTrigger) { active.push("lb".into()); }
                if p(Button::RightTrigger) { active.push("rb".into()); }
                if p(Button::Start) { active.push("start".into()); }
                if p(Button::Select) { active.push("back".into()); }
            }
            active.sort();
            active.dedup();

            let now = Instant::now();
            for b in &active {
                match next_fire.get(b) {
                    None => {
                        on_input(b);
                        let next = if is_directional(b) {
                            now + REPEAT_DELAY
                        } else {
                            now + Duration::from_secs(3600)
                        };
                        next_fire.insert(b.clone(), next);
                    }
                    Some(&t) if now >= t => {
                        on_input(b);
                        next_fire.insert(b.clone(), now + REPEAT_RATE);
                    }
                    _ => {}
                }
            }
            next_fire.retain(|k, _| active.contains(k));
            std::thread::sleep(POLL);
        }
    });
}
