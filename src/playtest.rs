//! Playtesting: bots play a game headless and report what a person would notice. Errors, a
//! player stuck against a wall or falling out of the level, a game no bot could win, a score that
//! never moves, input that does nothing, a game that's lost before anyone can react.
//!
//! Three bots, each for `seconds` of game time:
//! - `idle`: presses nothing (does the game wait for the player, or punish them at once?);
//! - `explorer`: heads for what the rules reward (collide rules that don't lose), jumps when
//!   blocked or when the goal is above, steps around what the rules punish; taps tappable things;
//! - `random`: holds random keys of the game for random stretches (seeded, so repeatable).
//!
//! Everything is deterministic: the same game gives the same report.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::format::{Action, Behaviour, Event};
use crate::world::{Frame, Input, Status, Uid, DT};
use crate::Engine;

fn thirty() -> f32 {
    30.0
}
fn five() -> f32 {
    5.0
}

#[derive(Clone, Debug, Deserialize)]
pub struct Options {
    /// Game seconds each bot plays (at most 300).
    #[serde(default = "thirty")]
    pub seconds: f32,
    /// Keep a snapshot every this many seconds (and the last frame), at most 8 per run.
    #[serde(default = "five")]
    pub snapshot_every: f32,
    /// Which bots: any of "idle", "explorer", "random" (default all three).
    #[serde(default)]
    pub bots: Vec<String>,
}

impl Default for Options {
    fn default() -> Self {
        Options { seconds: 30.0, snapshot_every: 5.0, bots: vec![] }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Snapshot {
    pub time: f64,
    pub frame: Frame,
}

#[derive(Clone, Debug, Serialize, Default)]
pub struct PlayerTrace {
    pub start: [f32; 2],
    pub end: [f32; 2],
    /// 32 px cells the player's centre passed through.
    pub cells: usize,
    pub farthest: f32,
    /// Longest stretch (seconds) the bot pressed movement keys and the player didn't move.
    pub stuck_seconds: f32,
    pub stuck_at: Option<[f32; 2]>,
    /// Where the player left the bottom of the level, if they did.
    pub fell_at: Option<[f32; 2]>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Run {
    pub bot: String,
    pub seconds: f64,
    pub status: Status,
    pub message: Option<String>,
    pub vars: BTreeMap<String, Value>,
    /// When each variable first changed (seconds).
    pub first_change: BTreeMap<String, f64>,
    pub scenes: Vec<String>,
    pub player: Option<PlayerTrace>,
    pub errors: Vec<String>,
    pub snapshots: Vec<Snapshot>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Report {
    pub runs: Vec<Run>,
    /// Plain-language observations, most important first.
    pub findings: Vec<String>,
    /// Keys the game listens to (behaviours and key rules).
    pub keys: Vec<String>,
    pub can_win: bool,
    pub can_lose: bool,
}

/// What the rules say about touching things, from the player's side.
struct Reading {
    player_tags: Vec<String>,
    goals: BTreeSet<String>,
    hazards: BTreeSet<String>,
    tap_tags: Vec<Option<String>>,
    keys: BTreeSet<String>,
    can_win: bool,
    can_lose: bool,
}

fn has_lose(actions: &[Action]) -> bool {
    actions.iter().any(|a| matches!(a, Action::Lose { .. } | Action::Restart))
}

fn read_game(e: &Engine) -> Reading {
    let w = e.world();
    let mut keys = BTreeSet::new();
    let mut player_tags = vec![];
    for ent in w.entities.iter().filter(|x| x.alive) {
        for b in &ent.behaviours {
            match b {
                Behaviour::Platformer { .. } => {
                    keys.extend(["left", "right", "up", "space"].map(String::from));
                    if player_tags.is_empty() {
                        player_tags = ent.tags.clone();
                    }
                }
                Behaviour::Topdown { .. } => {
                    keys.extend(["left", "right", "up", "down"].map(String::from));
                    if player_tags.is_empty() {
                        player_tags = ent.tags.clone();
                    }
                }
                _ => {}
            }
        }
    }
    if player_tags.is_empty() && w.entities.iter().any(|x| x.alive && x.has("player")) {
        player_tags = vec!["player".into()];
    }
    let (mut goals, mut hazards, mut tap_tags) = (BTreeSet::new(), BTreeSet::new(), vec![]);
    let (mut can_win, mut can_lose) = (false, false);
    let all_rules: Vec<_> = w.game.scenes.values().flat_map(|s| s.rules.iter()).collect();
    for r in &all_rules {
        can_win |= r.actions.iter().any(|a| matches!(a, Action::Win { .. }));
        can_lose |= r.actions.iter().any(|a| matches!(a, Action::Lose { .. }));
        match &r.when {
            Event::Key { key, .. } => {
                keys.insert(key.to_lowercase());
            }
            Event::Tap { tag } => tap_tags.push(tag.clone()),
            Event::Collide { a, b } => {
                let other = if player_tags.contains(a) {
                    Some(b)
                } else if player_tags.contains(b) {
                    Some(a)
                } else {
                    None
                };
                if let Some(o) = other {
                    if has_lose(&r.actions) || r.actions.iter().any(|x| matches!(x, Action::Add { value, .. } if *value < 0.0)) {
                        hazards.insert(o.clone());
                    } else {
                        goals.insert(o.clone());
                    }
                }
            }
            _ => {}
        }
    }
    // Scripts can win or lose too.
    let scripted = w.game.scripts.values().any(|s| s.contains("win(") || s.contains("lose("));
    if scripted {
        can_win |= w.game.scripts.values().any(|s| s.contains("win("));
        can_lose |= w.game.scripts.values().any(|s| s.contains("lose("));
    }
    Reading { player_tags, goals, hazards, tap_tags, keys, can_win, can_lose }
}

/// Seeded xorshift for the random bot.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn range(&mut self, lo: u64, hi: u64) -> u64 {
        lo + self.next() % (hi - lo).max(1)
    }
}

struct Bot {
    kind: String,
    rng: Rng,
    held: Vec<String>,
    hold_left: u32,
    jump_frame: bool,
    last_x: Vec<f32>,
    tap_wait: u32,
}

impl Bot {
    fn new(kind: &str, seed: u64) -> Bot {
        Bot { kind: kind.into(), rng: Rng(seed | 1), held: vec![], hold_left: 0, jump_frame: false, last_x: vec![], tap_wait: 0 }
    }

    fn input(&mut self, e: &Engine, r: &Reading, player: Option<Uid>) -> Input {
        let mut input = Input::default();
        match self.kind.as_str() {
            "idle" => {}
            "random" => {
                if self.hold_left == 0 {
                    let keys: Vec<&String> = r.keys.iter().collect();
                    self.held.clear();
                    if !keys.is_empty() {
                        for _ in 0..self.rng.range(1, 3) {
                            self.held.push(keys[self.rng.range(0, keys.len() as u64) as usize].clone());
                        }
                    }
                    self.hold_left = self.rng.range(8, 45) as u32;
                }
                self.hold_left -= 1;
                input.keys = self.held.iter().cloned().collect();
                if !r.tap_tags.is_empty() && self.rng.range(0, 20) == 0 {
                    let size = e.world().game.size;
                    input.pointer = Some([self.rng.range(0, size[0] as u64) as f32, self.rng.range(0, size[1] as u64) as f32]);
                    input.pointer_down = true;
                }
            }
            _ => self.explore(e, r, player, &mut input),
        }
        input
    }

    fn explore(&mut self, e: &Engine, r: &Reading, player: Option<Uid>, input: &mut Input) {
        let w = e.world();
        // Tap games: tap the nearest tappable thing every quarter second.
        if player.is_none() {
            if self.tap_wait > 0 {
                self.tap_wait -= 1;
                return;
            }
            let tags: Vec<&String> = r.tap_tags.iter().flatten().collect();
            let target = w.entities.iter().filter(|x| x.alive && (tags.is_empty() || tags.iter().any(|t| x.has(t)))).find(|x| !x.hud);
            if let Some(t) = target {
                let c = t.center();
                input.pointer = Some([c[0] - w.camera[0], c[1] - w.camera[1]]);
                input.pointer_down = true;
            } else if r.tap_tags.iter().any(|t| t.is_none()) {
                input.pointer = Some([w.game.size[0] / 2.0, w.game.size[1] / 2.0]);
                input.pointer_down = true;
            }
            self.tap_wait = 15;
            return;
        }
        let Some(p) = player.and_then(|u| w.get(u)) else { return };
        let me = p.center();
        let platformer = p.behaviours.iter().any(|b| matches!(b, Behaviour::Platformer { .. }));
        let nearest = |tags: &BTreeSet<String>| {
            w.entities
                .iter()
                .filter(|x| x.alive && x.uid != p.uid && tags.iter().any(|t| x.has(t)))
                .map(|x| (x.center(), ((x.center()[0] - me[0]).powi(2) + (x.center()[1] - me[1]).powi(2)).sqrt()))
                .min_by(|a, b| a.1.total_cmp(&b.1))
        };
        let goal = nearest(&r.goals);
        let hazard = nearest(&r.hazards);
        // No known goal: sweep right then left.
        let tx = goal.map(|g| g.0[0]).unwrap_or_else(|| if (w.frame / 240) % 2 == 0 { me[0] + 100.0 } else { me[0] - 100.0 });
        let ty = goal.map(|g| g.0[1]).unwrap_or(me[1]);
        let dx = tx - me[0];
        let dy = ty - me[1];
        let mut horiz = if dx > 4.0 { Some("right") } else if dx < -4.0 { Some("left") } else { None };
        let mut vert = if dy > 4.0 { Some("down") } else if dy < -4.0 { Some("up") } else { None };
        let mut jump = false;
        if let Some((hc, hd)) = hazard {
            if hd < 56.0 {
                if platformer {
                    jump = true;
                } else if (hc[0] - me[0]).abs() > (hc[1] - me[1]).abs() {
                    vert = Some(if hc[1] > me[1] { "up" } else { "down" });
                } else {
                    horiz = Some(if hc[0] > me[0] { "left" } else { "right" });
                }
            }
        }
        // Blocked: pressing sideways and not moving for 12 frames.
        self.last_x.push(me[0]);
        if self.last_x.len() > 12 {
            self.last_x.remove(0);
        }
        let blocked = self.last_x.len() == 12 && (self.last_x[11] - self.last_x[0]).abs() < 1.0 && horiz.is_some();
        if platformer && (blocked || (dy < -24.0 && dx.abs() < 96.0)) {
            jump = true;
        }
        if let Some(h) = horiz {
            input.keys.insert(h.into());
        }
        if platformer {
            // A jump is a press: down one frame, up the next.
            if jump && !self.jump_frame {
                input.keys.insert("up".into());
                self.jump_frame = true;
            } else {
                self.jump_frame = false;
            }
        } else if let Some(v) = vert {
            input.keys.insert(v.into());
        }
    }
}

const CELL: f32 = 32.0;

fn find_player(e: &Engine, r: &Reading) -> Option<Uid> {
    let w = e.world();
    w.entities
        .iter()
        .find(|x| x.alive && x.behaviours.iter().any(|b| matches!(b, Behaviour::Platformer { .. } | Behaviour::Topdown { .. })))
        .or_else(|| w.entities.iter().find(|x| x.alive && !r.player_tags.is_empty() && r.player_tags.iter().all(|t| x.has(t))))
        .map(|x| x.uid)
}

fn run_bot(json: &str, kind: &str, opts: &Options) -> Result<(Run, Reading), Vec<String>> {
    let mut e = Engine::load(json)?;
    // Start the scene so its entities exist before reading the rules.
    let first = e.step(Input::default());
    let reading = read_game(&e);
    let frames = (opts.seconds.clamp(1.0, 300.0) / DT).round() as u64;
    let every = (opts.snapshot_every.max(1.0) / DT).round() as u64;
    let mut bot = Bot::new(kind, 0x5eed_0f0e_u64 ^ kind.len() as u64);
    let start_vars = e.world().vars.clone();
    let mut first_change = BTreeMap::new();
    let mut scenes = vec![e.world().scene_name.clone()];
    let mut errors: Vec<String> = first.errors.clone();
    let mut snapshots = vec![Snapshot { time: 0.0, frame: first.clone() }];
    let mut trace: Option<PlayerTrace> = None;
    let mut cells = HashSet::new();
    let (mut still_frames, mut was) = (0u32, None::<[f32; 2]>);
    let mut last = first;
    for _ in 1..frames {
        let player = find_player(&e, &reading);
        let input = bot.input(&e, &reading, player);
        let moving_keys = input.keys.iter().any(|k| ["left", "right", "up", "down"].contains(&k.as_str()));
        let f = e.step(input);
        {
            let w = e.world();
            for (k, v) in &w.vars {
                if start_vars.get(k) != Some(v) && !first_change.contains_key(k) {
                    first_change.insert(k.clone(), w.time);
                }
            }
            if scenes.last() != Some(&w.scene_name) {
                scenes.push(w.scene_name.clone());
            }
            if let Some(p) = player.and_then(|u| w.get(u)) {
                let c = p.center();
                let t = trace.get_or_insert_with(|| PlayerTrace { start: c, ..Default::default() });
                t.end = c;
                cells.insert(((c[0] / CELL).floor() as i32, (c[1] / CELL).floor() as i32));
                t.farthest = t.farthest.max(((c[0] - t.start[0]).powi(2) + (c[1] - t.start[1]).powi(2)).sqrt());
                let moved = was.map(|o| ((o[0] - c[0]).abs() + (o[1] - c[1]).abs()) > 0.25).unwrap_or(true);
                if moving_keys && !moved {
                    still_frames += 1;
                    let secs = still_frames as f32 * DT;
                    if secs > t.stuck_seconds {
                        t.stuck_seconds = secs;
                        t.stuck_at = Some(c);
                    }
                } else if moved {
                    still_frames = 0;
                }
                was = Some(c);
                let b = w.bounds();
                if t.fell_at.is_none() && c[1] > b[1] + b[3] + 48.0 {
                    t.fell_at = Some([c[0], b[1] + b[3]]);
                }
            }
        }
        for err in &f.errors {
            if !errors.contains(err) {
                errors.push(err.clone());
            }
        }
        let done = f.status != Status::Running || e.halted.is_some();
        if f.frame % every == 0 && snapshots.len() < 8 {
            snapshots.push(Snapshot { time: f.time, frame: f.clone() });
        }
        last = f;
        if done {
            break;
        }
    }
    if snapshots.last().map(|s| s.frame.frame) != Some(last.frame) {
        if snapshots.len() >= 8 {
            snapshots.pop();
        }
        snapshots.push(Snapshot { time: last.time, frame: last.clone() });
    }
    if let Some(t) = trace.as_mut() {
        t.cells = cells.len();
    }
    let w = e.world();
    Ok((
        Run { bot: kind.into(), seconds: w.time, status: w.status.clone(), message: w.message.clone(), vars: w.vars.clone(), first_change, scenes, player: trace, errors, snapshots },
        reading,
    ))
}

fn fmt_s(s: f64) -> String {
    format!("{s:.1} s")
}

/// Bots play the game; their runs and what they found.
pub fn playtest(json: &str, opts: &Options) -> Result<Report, Vec<String>> {
    let bots: Vec<String> = if opts.bots.is_empty() { vec!["idle".into(), "explorer".into(), "random".into()] } else { opts.bots.clone() };
    let mut runs = vec![];
    let mut reading = None;
    for b in &bots {
        let (run, r) = run_bot(json, b, opts)?;
        runs.push(run);
        reading.get_or_insert(r);
    }
    let r = reading.expect("at least one bot");
    let mut findings = vec![];
    let errors: BTreeSet<&String> = runs.iter().flat_map(|x| x.errors.iter()).collect();
    for e in &errors {
        findings.push(format!("Error: {e}"));
    }
    let get = |b: &str| runs.iter().find(|x| x.bot == b);
    if let Some(idle) = get("idle") {
        if idle.status == Status::Lost && idle.seconds < 3.0 {
            findings.push(format!("With no input the game is lost after {}: players get no time to react.", fmt_s(idle.seconds)));
        }
        if idle.status == Status::Won {
            findings.push(format!("With no input the game is won after {}: it may play itself.", fmt_s(idle.seconds)));
        }
    }
    if r.can_win && !runs.iter().any(|x| x.status == Status::Won) {
        let best = get("explorer").or(runs.first()).expect("a run");
        let progress = best.vars.iter().filter(|(k, _)| best.first_change.contains_key(*k)).map(|(k, v)| format!("{k} {v}")).collect::<Vec<_>>().join(", ");
        findings.push(format!(
            "No bot won within {}{}. It may be too hard, or the way to win is hard to find.",
            fmt_s(opts.seconds as f64),
            if progress.is_empty() { String::new() } else { format!(" (best: {progress})") }
        ));
    }
    if !r.can_win && !r.can_lose {
        findings.push("Nothing in the rules or scripts wins or loses the game, so it never ends.".into());
    }
    for run in &runs {
        if let Some(t) = &run.player {
            if let Some(at) = t.fell_at {
                findings.push(format!("{} bot: the player fell out of the level near x {:.0} and nothing caught it.", run.bot, at[0]));
            }
            if t.stuck_seconds >= 2.0 && run.bot != "idle" {
                let at = t.stuck_at.unwrap_or_default();
                findings.push(format!("{} bot: the player was stuck for {} near ({:.0}, {:.0}).", run.bot, fmt_s(t.stuck_seconds as f64), at[0], at[1]));
            }
        }
    }
    let tracked: Vec<&String> = ["score", "points", "coins", "gems"].iter().filter_map(|k| runs[0].vars.keys().find(|v| v.as_str() == *k)).collect();
    for k in tracked {
        if !runs.iter().any(|x| x.first_change.contains_key(k)) {
            findings.push(format!("“{k}” never changed in any run."));
        }
    }
    if let (Some(idle), Some(rand)) = (get("idle"), get("random")) {
        let same_pos = match (&idle.player, &rand.player) {
            (Some(a), Some(b)) => (a.end[0] - b.end[0]).abs() < 1.0 && (a.end[1] - b.end[1]).abs() < 1.0,
            (None, None) => true,
            _ => false,
        };
        if !r.keys.is_empty() && same_pos && idle.vars == rand.vars && idle.status == rand.status {
            findings.push("Pressing keys didn't change anything compared with pressing nothing.".into());
        }
    }
    if r.keys.is_empty() && r.tap_tags.is_empty() {
        findings.push("The game doesn't listen to any keys or taps, so there's nothing to play.".into());
    }
    if findings.is_empty() {
        let won = runs.iter().filter(|x| x.status == Status::Won).map(|x| format!("{} in {}", x.bot, fmt_s(x.seconds))).collect::<Vec<_>>();
        findings.push(if won.is_empty() { "No problems found.".into() } else { format!("No problems found. Won by: {}.", won.join(", ")) });
    }
    Ok(Report { runs, findings, keys: r.keys.into_iter().collect(), can_win: r.can_win, can_lose: r.can_lose })
}
