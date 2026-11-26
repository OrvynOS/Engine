//! Replays: the player records its input (what changed on which frame), and anyone can replay it
//! with the same game to get the same result. Leaderboards check scores this way: a score counts
//! only if replaying its input gives it.
//!
//! Games with TypeScript/JavaScript scripts can't be checked (those run in the browser, not
//! here); the replay says so instead of guessing.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::world::{Input, Status};
use crate::Engine;

/// Longest replay checked: 20 minutes at 60 frames a second.
pub const MAX_FRAMES: u64 = 20 * 60 * 60;

/// What the player did, as changes: `keys` are `[frame, "+left"]` (pressed) or `[frame, "-left"]`
/// (released); `pointer` entries are `[frame, x, y, down]` in game pixels (`x` null: no pointer).
/// `frames` is how many engine steps the run took.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct InputLog {
    #[serde(default)]
    pub v: u32,
    pub frames: u64,
    #[serde(default)]
    pub keys: Vec<(u64, String)>,
    #[serde(default)]
    pub pointer: Vec<(u64, Option<f32>, Option<f32>, bool)>,
    /// Saved variables the run started with (set before the first frame).
    #[serde(default)]
    pub start: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Replayed {
    pub frames: u64,
    pub time: f64,
    pub status: Status,
    pub message: Option<String>,
    pub vars: BTreeMap<String, Value>,
    /// False when the result can't be trusted (browser scripts), with why.
    pub verifiable: bool,
    pub reason: Option<String>,
}

pub fn replay(json: &str, log: &InputLog) -> Result<Replayed, Vec<String>> {
    if log.frames > MAX_FRAMES {
        return Err(vec![format!("the run is longer than {} minutes", MAX_FRAMES / 3600)]);
    }
    let mut e = Engine::load(json)?;
    let browser_scripts = e.world().game.scripts.keys().any(|k| k.ends_with(".ts") || k.ends_with(".js"));
    let saved: Vec<String> = e.world().game.save.clone();
    for (k, v) in &log.start {
        if !saved.contains(k) {
            return Err(vec![format!("the run starts with \"{k}\", which the game doesn't save")]);
        }
        e.api(&serde_json::json!({ "op": "set", "name": k, "value": v }));
    }
    let mut input = Input::default();
    let (mut ki, mut pi) = (0usize, 0usize);
    let mut keys = log.keys.clone();
    keys.sort_by_key(|k| k.0);
    let mut pointer = log.pointer.clone();
    pointer.sort_by_key(|p| p.0);
    for frame in 0..log.frames {
        while ki < keys.len() && keys[ki].0 == frame {
            let k = &keys[ki].1;
            if let Some(name) = k.strip_prefix('+') {
                input.keys.insert(name.to_string());
            } else if let Some(name) = k.strip_prefix('-') {
                input.keys.remove(name);
            }
            ki += 1;
        }
        while pi < pointer.len() && pointer[pi].0 == frame {
            let (_, x, y, down) = pointer[pi];
            input.pointer = match (x, y) {
                (Some(x), Some(y)) => Some([x, y]),
                _ => None,
            };
            input.pointer_down = down;
            pi += 1;
        }
        let f = e.step(input.clone());
        if f.status != Status::Running || e.halted.is_some() {
            break;
        }
    }
    let w = e.world();
    Ok(Replayed {
        frames: w.frame,
        time: w.time,
        status: w.status.clone(),
        message: w.message.clone(),
        vars: w.vars.clone(),
        verifiable: !browser_scripts,
        reason: browser_scripts.then(|| "the game has TypeScript/JavaScript scripts, which run only in the browser".to_string()),
    })
}
