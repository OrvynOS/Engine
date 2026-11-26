//! The game file (`orvyn-game/1`): one JSON document a human (in the editor) or an agent writes.
//!
//! ```json
//! { "format": "orvyn-game/1", "title": "Coin Run", "size": [480, 270], "start": "level1",
//!   "vars": { "score": 0 },
//!   "prefabs": { "coin": { "tags": ["coin"], "size": [12, 12], "shape": "circle", "color": "#f5c542", "sensor": true } },
//!   "scenes": { "level1": { "gravity": [0, 900], "entities": [ ... ], "tilemap": { ... }, "rules": [ ... ] } },
//!   "scripts": { "main.rhai": "fn on_start() { ... }" } }
//! ```

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const FORMAT: &str = "orvyn-game/1";

fn default_size() -> [f32; 2] {
    [480.0, 270.0]
}
fn black() -> String {
    "#000000".into()
}
fn yes() -> bool {
    true
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Game {
    pub format: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    /// The game's own resolution; the player scales it to fit the screen.
    #[serde(default = "default_size")]
    pub size: [f32; 2],
    #[serde(default = "black")]
    pub background: String,
    /// Seed for `random` (rules and scripts): the same inputs replay the same game.
    #[serde(default)]
    pub seed: u64,
    pub start: String,
    /// Game-wide variables (score, lives…), kept across scenes.
    #[serde(default)]
    pub vars: BTreeMap<String, Value>,
    #[serde(default)]
    pub assets: Assets,
    #[serde(default)]
    pub prefabs: BTreeMap<String, EntityDef>,
    pub scenes: BTreeMap<String, Scene>,
    /// `*.rhai` run in the engine (everywhere); `*.ts`/`*.js` run in the browser player.
    #[serde(default)]
    pub scripts: BTreeMap<String, String>,
    /// On-screen controls for touch screens: which keys to show as buttons.
    #[serde(default)]
    pub touch: Option<Touch>,
    /// A leaderboard on Orvyn: each player's best `var` when a game ends (`"$time"`: seconds to
    /// win). Scores are checked by replaying the player's input, so the engine must run the whole
    /// game (rules and Rhai, no TypeScript/JavaScript).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leaderboard: Option<Leaderboard>,
    /// Variables kept for each player between plays (progress, unlocked levels, settings).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub save: Vec<String>,
}

fn high() -> String {
    "high".into()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Leaderboard {
    /// A game variable, or "$time" (seconds to win; only won runs count).
    pub var: String,
    /// "high" (more is better) or "low" (less is better, e.g. time).
    #[serde(default = "high")]
    pub order: String,
    /// What the column is called ("Score", "Best time").
    #[serde(default)]
    pub label: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Touch {
    /// "dpad" (left/right/up/down), "leftright", or none.
    #[serde(default)]
    pub pad: Option<String>,
    /// Extra buttons, e.g. ["space"] shown as "A".
    #[serde(default)]
    pub buttons: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Assets {
    #[serde(default)]
    pub images: BTreeMap<String, ImageAsset>,
    #[serde(default)]
    pub sounds: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ImageAsset {
    /// A file of the game (or a data: URL).
    pub src: String,
    /// Sprite sheets: one frame's size; frames are numbered left to right, top to bottom.
    #[serde(default)]
    pub frame: Option<[f32; 2]>,
    /// Named animations: frames and speed.
    #[serde(default)]
    pub anims: BTreeMap<String, Anim>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Anim {
    pub frames: Vec<u32>,
    #[serde(default = "fps8")]
    pub fps: f32,
    #[serde(default = "yes", rename = "loop")]
    pub looped: bool,
}
fn fps8() -> f32 {
    8.0
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Scene {
    #[serde(default)]
    pub gravity: [f32; 2],
    #[serde(default)]
    pub background: Option<String>,
    /// The playable area (default: the screen). The camera stays inside it.
    #[serde(default)]
    pub bounds: Option<[f32; 4]>,
    #[serde(default)]
    pub camera: Camera,
    #[serde(default)]
    pub entities: Vec<EntityDef>,
    #[serde(default)]
    pub tilemap: Option<Tilemap>,
    #[serde(default)]
    pub rules: Vec<Rule>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Camera {
    /// Follow the first entity with this tag.
    #[serde(default)]
    pub follow: Option<String>,
    #[serde(default)]
    pub zoom: Option<f32>,
}

/// A grid: each character of `rows` is a tile; `legend` says what each character is.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Tilemap {
    pub tile: [f32; 2],
    #[serde(default)]
    pub origin: [f32; 2],
    pub rows: Vec<String>,
    pub legend: BTreeMap<String, TileDef>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TileDef {
    /// The name of a prefab placed at the tile.
    Prefab(String),
    /// A solid (or decorative) tile.
    Tile {
        #[serde(default)]
        color: Option<String>,
        #[serde(default)]
        image: Option<String>,
        #[serde(default)]
        frame: Option<u32>,
        #[serde(default = "yes")]
        solid: bool,
        #[serde(default)]
        tags: Vec<String>,
    },
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Sprite {
    pub image: String,
    #[serde(default)]
    pub frame: u32,
    #[serde(default)]
    pub anim: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Text {
    pub value: String,
    #[serde(default = "sixteen")]
    pub size: f32,
    #[serde(default)]
    pub color: Option<String>,
    /// "left", "center" (default) or "right".
    #[serde(default)]
    pub align: Option<String>,
}
fn sixteen() -> f32 {
    16.0
}

/// An entity (or a prefab: the same fields, placed later with `spawn`).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct EntityDef {
    #[serde(default)]
    pub id: Option<String>,
    /// Start from a prefab, then apply this entity's fields over it.
    #[serde(default)]
    pub prefab: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub pos: Option<[f32; 2]>,
    #[serde(default)]
    pub size: Option<[f32; 2]>,
    /// "rect" (default) or "circle".
    #[serde(default)]
    pub shape: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub sprite: Option<Sprite>,
    #[serde(default)]
    pub text: Option<Text>,
    /// "none" (default), "static" (a wall), "dynamic" (falls, collides), "kinematic" (moves, not pushed).
    #[serde(default)]
    pub body: Option<String>,
    /// Overlaps are reported (collide rules) but nothing is pushed apart.
    #[serde(default)]
    pub sensor: Option<bool>,
    #[serde(default)]
    pub vel: Option<[f32; 2]>,
    #[serde(default)]
    pub layer: Option<i32>,
    /// Drawn in screen space (score, buttons), not moved by the camera.
    #[serde(default)]
    pub hud: Option<bool>,
    #[serde(default)]
    pub visible: Option<bool>,
    #[serde(default)]
    pub rotation: Option<f32>,
    #[serde(default)]
    pub opacity: Option<f32>,
    #[serde(default)]
    pub behaviours: Vec<Behaviour>,
    /// The entity's own variables (hp, speed…).
    #[serde(default)]
    pub vars: BTreeMap<String, Value>,
}

/// Ready-made movement and behaviour, no code needed.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Behaviour {
    /// Left/right to run, up or space to jump (only on the ground).
    Platformer {
        #[serde(default = "speed160")]
        speed: f32,
        #[serde(default = "jump360")]
        jump: f32,
    },
    /// Arrow keys/WASD move in four directions.
    Topdown {
        #[serde(default = "speed160")]
        speed: f32,
    },
    /// Back and forth along x (or y) between two offsets, turning at walls.
    Patrol {
        #[serde(default = "speed60")]
        speed: f32,
        #[serde(default = "range80")]
        range: f32,
        #[serde(default)]
        vertical: bool,
    },
    /// Move toward the first entity with `target` tag.
    Follow {
        target: String,
        #[serde(default = "speed60")]
        speed: f32,
    },
    /// Reverse velocity at the bounds of the scene.
    Bounce,
    /// Leaving one edge comes back at the other.
    Wrap,
    /// Turn continuously (degrees per second).
    Spin {
        #[serde(default = "deg90")]
        speed: f32,
    },
    /// Destroyed after `seconds`.
    Lifetime { seconds: f32 },
    /// Destroyed when it leaves the scene's bounds.
    Offscreen,
    /// Follow the pointer (mouse or finger) along x, y or both.
    Pointer {
        #[serde(default)]
        axis: Option<String>,
    },
}
fn speed160() -> f32 {
    160.0
}
fn jump360() -> f32 {
    360.0
}
fn speed60() -> f32 {
    60.0
}
fn range80() -> f32 {
    80.0
}
fn deg90() -> f32 {
    90.0
}

/// A no-code rule: when something happens, if conditions hold, do things.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Rule {
    pub when: Event,
    #[serde(default, rename = "if")]
    pub conditions: Vec<Condition>,
    #[serde(rename = "do")]
    pub actions: Vec<Action>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    Start,
    Tick,
    Every { seconds: f32 },
    /// A key: "pressed" (this frame), "down" (held), or "released".
    Key {
        key: String,
        #[serde(default = "pressed")]
        state: String,
    },
    /// A tap or click: on entities with `tag`, or anywhere.
    Tap {
        #[serde(default)]
        tag: Option<String>,
    },
    /// Two entities with these tags touch (once, when they start touching).
    Collide { a: String, b: String },
    /// A variable reaches a value (checked every frame; fires when it becomes true).
    Var { name: String, op: String, value: Value },
}
fn pressed() -> String {
    "pressed".into()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Condition {
    Var { var: String, op: String, value: Value },
    Count { count: String, op: String, value: f64 },
    Chance { chance: f64 },
}

/// Who an action applies to: "self"/"a" (the first entity of the event), "b" (the other),
/// "tag:<name>" (all with a tag), or an entity id.
pub type Target = String;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Action {
    /// Set a variable (game-wide, or `on` an entity's own).
    Set {
        var: String,
        value: Value,
        #[serde(default)]
        on: Option<Target>,
    },
    /// Add to a number variable (negative to subtract).
    Add {
        var: String,
        value: f64,
        #[serde(default)]
        on: Option<Target>,
    },
    /// Place a prefab at a position, or at an entity (plus an offset).
    Spawn {
        prefab: String,
        #[serde(default)]
        at: Option<Target>,
        #[serde(default)]
        pos: Option<[f32; 2]>,
        #[serde(default)]
        offset: Option<[f32; 2]>,
        #[serde(default)]
        vel: Option<[f32; 2]>,
    },
    Destroy {
        #[serde(default = "self_target")]
        target: Target,
    },
    Move {
        #[serde(default = "self_target")]
        target: Target,
        #[serde(default)]
        to: Option<[f32; 2]>,
        #[serde(default)]
        by: Option<[f32; 2]>,
    },
    Velocity {
        #[serde(default = "self_target")]
        target: Target,
        vel: [f32; 2],
    },
    /// Show/hide, change colour, text, sprite animation.
    Change {
        #[serde(default = "self_target")]
        target: Target,
        #[serde(default)]
        visible: Option<bool>,
        #[serde(default)]
        color: Option<String>,
        #[serde(default)]
        text: Option<String>,
        #[serde(default)]
        anim: Option<String>,
    },
    Sound { sound: String },
    Shake {
        #[serde(default = "shake")]
        strength: f32,
        #[serde(default = "shake_time")]
        seconds: f32,
    },
    /// Go to another scene (game-wide vars are kept).
    Scene { scene: String },
    Restart,
    /// End the game: won or lost, with a message.
    Win {
        #[serde(default)]
        message: Option<String>,
    },
    Lose {
        #[serde(default)]
        message: Option<String>,
    },
    /// Call a script function (Rhai in the engine, or TS/JS in the player) with the event's entities.
    Call { function: String },
}
fn self_target() -> Target {
    "self".into()
}
fn shake() -> f32 {
    6.0
}
fn shake_time() -> f32 {
    0.3
}

impl EntityDef {
    /// `self` with fields missing taken from `base` (a prefab). Tags, behaviours and vars merge.
    pub fn over(&self, base: &EntityDef) -> EntityDef {
        let mut tags = base.tags.clone();
        for t in &self.tags {
            if !tags.contains(t) {
                tags.push(t.clone());
            }
        }
        let mut vars = base.vars.clone();
        vars.extend(self.vars.clone());
        EntityDef {
            id: self.id.clone(),
            prefab: None,
            tags,
            pos: self.pos.or(base.pos),
            size: self.size.or(base.size),
            shape: self.shape.clone().or(base.shape.clone()),
            color: self.color.clone().or(base.color.clone()),
            sprite: self.sprite.clone().or(base.sprite.clone()),
            text: self.text.clone().or(base.text.clone()),
            body: self.body.clone().or(base.body.clone()),
            sensor: self.sensor.or(base.sensor),
            vel: self.vel.or(base.vel),
            layer: self.layer.or(base.layer),
            hud: self.hud.or(base.hud),
            visible: self.visible.or(base.visible),
            rotation: self.rotation.or(base.rotation),
            opacity: self.opacity.or(base.opacity),
            behaviours: if self.behaviours.is_empty() { base.behaviours.clone() } else { self.behaviours.clone() },
            vars,
        }
    }
}

