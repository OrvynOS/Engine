//! The running game: entities, variables, input, physics, behaviours and rule actions. Everything
//! here is deterministic: the same game, seed and inputs give the same frames.

use std::collections::{BTreeMap, HashSet};

use serde::Serialize;
use serde_json::{json, Value};

use crate::format::*;

pub type Uid = u32;

#[derive(Clone, Debug)]
pub struct Entity {
    pub uid: Uid,
    pub id: Option<String>,
    pub tags: Vec<String>,
    pub pos: [f32; 2],
    pub size: [f32; 2],
    pub circle: bool,
    pub color: Option<String>,
    pub sprite: Option<Sprite>,
    pub anim_time: f32,
    pub text: Option<Text>,
    pub body: BodyKind,
    pub sensor: bool,
    pub vel: [f32; 2],
    pub layer: i32,
    pub hud: bool,
    pub visible: bool,
    pub rotation: f32,
    pub opacity: f32,
    pub behaviours: Vec<Behaviour>,
    pub vars: BTreeMap<String, Value>,
    pub on_ground: bool,
    pub alive: bool,
    pub age: f32,
    pub origin: [f32; 2],
    pub dir: f32,
    pub flip: bool,
    /// Tiles: their frame of a tile image.
    pub tile_frame: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BodyKind {
    None,
    Static,
    Dynamic,
    Kinematic,
}

impl Entity {
    pub fn has(&self, tag: &str) -> bool {
        self.tags.iter().any(|t| t == tag)
    }
    fn solid(&self) -> bool {
        !self.sensor && matches!(self.body, BodyKind::Static | BodyKind::Kinematic)
    }
    pub fn center(&self) -> [f32; 2] {
        [self.pos[0] + self.size[0] / 2.0, self.pos[1] + self.size[1] / 2.0]
    }
}

#[derive(Clone, Debug, Default)]
pub struct Input {
    pub keys: HashSet<String>,
    pub pointer: Option<[f32; 2]>,
    pub pointer_down: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Running,
    Won,
    Lost,
}

/// One thing to draw, in screen pixels of the game's resolution.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "k")]
pub enum Draw {
    #[serde(rename = "r")]
    Rect { x: f32, y: f32, w: f32, h: f32, c: String, a: f32, rot: f32 },
    #[serde(rename = "o")]
    Circle { x: f32, y: f32, w: f32, h: f32, c: String, a: f32 },
    /// A whole image, or frame `f` of a sheet whose frames are `fw` x `fh`.
    #[serde(rename = "i")]
    Image { img: String, f: u32, fw: f32, fh: f32, x: f32, y: f32, w: f32, h: f32, a: f32, rot: f32, flip: bool },
    #[serde(rename = "t")]
    Text { s: String, x: f32, y: f32, w: f32, size: f32, c: String, align: String, a: f32 },
}

/// What the player needs for one frame.
#[derive(Clone, Debug, Serialize)]
pub struct Frame {
    pub frame: u64,
    pub time: f64,
    pub background: String,
    pub draws: Vec<Draw>,
    pub sounds: Vec<String>,
    pub status: Status,
    pub message: Option<String>,
    pub vars: BTreeMap<String, Value>,
    /// Functions for the player's TS/JS scripts to run (rules' `call` actions with no Rhai function).
    pub calls: Vec<HostCall>,
    /// New contacts this frame [a, b] (for TS/JS `onCollide`).
    pub contacts: Vec<[Uid; 2]>,
    pub logs: Vec<String>,
    pub errors: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct HostCall {
    pub function: String,
    pub a: Option<Uid>,
    pub b: Option<Uid>,
}

/// The entities an event happened to.
#[derive(Clone, Copy, Debug, Default)]
pub struct Ctx {
    pub a: Option<Uid>,
    pub b: Option<Uid>,
}

pub struct World {
    pub game: Game,
    pub scene_name: String,
    pub scene: Scene,
    pub entities: Vec<Entity>,
    pub next_uid: Uid,
    pub vars: BTreeMap<String, Value>,
    rng: u64,
    pub time: f64,
    pub frame: u64,
    pub input: Input,
    prev_keys: HashSet<String>,
    prev_pointer_down: bool,
    pub status: Status,
    pub message: Option<String>,
    pub sounds: Vec<String>,
    shake: (f32, f32),
    pub camera: [f32; 2],
    touching: HashSet<(Uid, Uid)>,
    pub contacts: Vec<[Uid; 2]>,
    /// Tags that appear in this scene's collide rules: those entities take part in contacts even
    /// without a body.
    collide_tags: HashSet<String>,
    every: Vec<f32>,
    var_was: Vec<bool>,
    pub pending_scene: Option<String>,
    pub pending_restart: bool,
    pub logs: Vec<String>,
    pub errors: Vec<String>,
    pub host_calls: Vec<HostCall>,
}

pub const DT: f32 = 1.0 / 60.0;

fn num(v: &Value) -> f64 {
    match v {
        Value::Number(n) => n.as_f64().unwrap_or(0.0),
        Value::Bool(b) => *b as i32 as f64,
        Value::String(s) => s.parse().unwrap_or(0.0),
        _ => 0.0,
    }
}

pub fn compare(a: &Value, op: &str, b: &Value) -> bool {
    match op {
        "==" | "=" => a == b || (a.is_number() && b.is_number() && num(a) == num(b)),
        "!=" => !(a == b || (a.is_number() && b.is_number() && num(a) == num(b))),
        ">" => num(a) > num(b),
        ">=" => num(a) >= num(b),
        "<" => num(a) < num(b),
        "<=" => num(a) <= num(b),
        _ => false,
    }
}

fn overlap(a: &Entity, b: &Entity, slack: f32) -> bool {
    if a.circle && b.circle {
        let (ca, cb) = (a.center(), b.center());
        // Radius: half the mean of width and height.
        let r = (a.size[0] + a.size[1]) / 4.0 + (b.size[0] + b.size[1]) / 4.0 + slack;
        let (dx, dy) = (ca[0] - cb[0], ca[1] - cb[1]);
        return dx * dx + dy * dy < r * r;
    }
    a.pos[0] < b.pos[0] + b.size[0] + slack && b.pos[0] < a.pos[0] + a.size[0] + slack && a.pos[1] < b.pos[1] + b.size[1] + slack && b.pos[1] < a.pos[1] + a.size[1] + slack
}

impl World {
    pub fn new(game: Game) -> World {
        let seed = if game.seed == 0 { 0x9E37_79B9_7F4A_7C15 } else { game.seed };
        let vars = game.vars.clone();
        let start = game.start.clone();
        let mut w = World {
            game,
            scene_name: String::new(),
            scene: Scene::default(),
            entities: Vec::new(),
            next_uid: 1,
            vars,
            rng: seed,
            time: 0.0,
            frame: 0,
            input: Input::default(),
            prev_keys: HashSet::new(),
            prev_pointer_down: false,
            status: Status::Running,
            message: None,
            sounds: Vec::new(),
            shake: (0.0, 0.0),
            camera: [0.0, 0.0],
            touching: HashSet::new(),
            contacts: Vec::new(),
            collide_tags: HashSet::new(),
            every: Vec::new(),
            var_was: Vec::new(),
            pending_scene: None,
            pending_restart: false,
            logs: Vec::new(),
            errors: Vec::new(),
            host_calls: Vec::new(),
        };
        w.load_scene(&start);
        w
    }

    /// Seeded random number in [0, 1).
    pub fn random(&mut self) -> f64 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng >> 11) as f64 / (1u64 << 53) as f64
    }

    pub fn bounds(&self) -> [f32; 4] {
        self.scene.bounds.unwrap_or([0.0, 0.0, self.game.size[0], self.game.size[1]])
    }

    pub fn load_scene(&mut self, name: &str) {
        let Some(scene) = self.game.scenes.get(name).cloned() else {
            self.errors.push(format!("no scene named {name}"));
            return;
        };
        self.scene_name = name.to_string();
        self.entities.clear();
        self.touching.clear();
        self.every = vec![0.0; scene.rules.len()];
        self.var_was = vec![false; scene.rules.len()];
        self.collide_tags = scene
            .rules
            .iter()
            .filter_map(|r| match &r.when {
                Event::Collide { a, b } => Some([a.clone(), b.clone()]),
                _ => None,
            })
            .flatten()
            .collect();
        self.scene = scene;
        if let Some(map) = self.scene.tilemap.clone() {
            self.load_tilemap(&map);
        }
        for def in self.scene.entities.clone() {
            self.spawn_def(&def, None);
        }
        self.camera = [self.bounds()[0], self.bounds()[1]];
    }

    fn load_tilemap(&mut self, map: &Tilemap) {
        for (row, line) in map.rows.iter().enumerate() {
            let chars: Vec<char> = line.chars().collect();
            let mut col = 0;
            while col < chars.len() {
                let key = chars[col].to_string();
                let pos = [map.origin[0] + col as f32 * map.tile[0], map.origin[1] + row as f32 * map.tile[1]];
                match map.legend.get(&key) {
                    Some(TileDef::Prefab(p)) => {
                        let def = EntityDef { prefab: Some(p.clone()), pos: Some(pos), ..Default::default() };
                        self.spawn_def(&def, None);
                        col += 1;
                    }
                    Some(TileDef::Tile { color, image, frame, solid, tags }) => {
                        // Merge a run of the same plain tile into one block.
                        let mut run = 1;
                        if image.is_none() {
                            while col + run < chars.len() && chars[col + run] == chars[col] {
                                run += 1;
                            }
                        }
                        let mut tags = tags.clone();
                        tags.push("tile".into());
                        let e = self.blank(pos, [map.tile[0] * run as f32, map.tile[1]]);
                        let uid = e.uid;
                        self.entities.push(Entity {
                            tags,
                            color: color.clone(),
                            sprite: image.as_ref().map(|i| Sprite { image: i.clone(), frame: frame.unwrap_or(0), anim: None }),
                            tile_frame: *frame,
                            body: if *solid { BodyKind::Static } else { BodyKind::None },
                            layer: -10,
                            ..e
                        });
                        let _ = uid;
                        col += run;
                    }
                    None => col += 1,
                }
            }
        }
    }

    fn blank(&mut self, pos: [f32; 2], size: [f32; 2]) -> Entity {
        let uid = self.next_uid;
        self.next_uid += 1;
        Entity {
            uid,
            id: None,
            tags: Vec::new(),
            pos,
            size,
            circle: false,
            color: None,
            sprite: None,
            anim_time: 0.0,
            text: None,
            body: BodyKind::None,
            sensor: false,
            vel: [0.0, 0.0],
            layer: 0,
            hud: false,
            visible: true,
            rotation: 0.0,
            opacity: 1.0,
            behaviours: Vec::new(),
            vars: BTreeMap::new(),
            on_ground: false,
            alive: true,
            age: 0.0,
            origin: pos,
            dir: 1.0,
            flip: false,
            tile_frame: None,
        }
    }

    /// Create an entity from a definition (prefab applied); `at` overrides its position.
    pub fn spawn_def(&mut self, def: &EntityDef, at: Option<[f32; 2]>) -> Option<Uid> {
        let def = match &def.prefab {
            Some(p) => match self.game.prefabs.get(p) {
                Some(base) => def.over(base),
                None => {
                    self.errors.push(format!("no prefab named {p}"));
                    return None;
                }
            },
            None => def.clone(),
        };
        let pos = at.or(def.pos).unwrap_or([0.0, 0.0]);
        let mut e = self.blank(pos, def.size.unwrap_or([16.0, 16.0]));
        e.id = def.id.clone();
        e.tags = def.tags.clone();
        e.circle = def.shape.as_deref() == Some("circle");
        e.color = def.color.clone();
        e.sprite = def.sprite.clone();
        e.text = def.text.clone();
        e.body = match def.body.as_deref() {
            Some("static") => BodyKind::Static,
            Some("dynamic") => BodyKind::Dynamic,
            Some("kinematic") => BodyKind::Kinematic,
            _ => BodyKind::None,
        };
        e.sensor = def.sensor.unwrap_or(false);
        e.vel = def.vel.unwrap_or([0.0, 0.0]);
        e.layer = def.layer.unwrap_or(0);
        e.hud = def.hud.unwrap_or(false);
        e.visible = def.visible.unwrap_or(true);
        e.rotation = def.rotation.unwrap_or(0.0);
        e.opacity = def.opacity.unwrap_or(1.0);
        e.behaviours = def.behaviours.clone();
        e.vars = def.vars.clone();
        let uid = e.uid;
        self.entities.push(e);
        Some(uid)
    }

    pub fn get(&self, uid: Uid) -> Option<&Entity> {
        self.entities.iter().find(|e| e.uid == uid && e.alive)
    }

    pub fn get_mut(&mut self, uid: Uid) -> Option<&mut Entity> {
        self.entities.iter_mut().find(|e| e.uid == uid && e.alive)
    }

    pub fn with_tag(&self, tag: &str) -> Vec<Uid> {
        self.entities.iter().filter(|e| e.alive && e.has(tag)).map(|e| e.uid).collect()
    }

    /// "self"/"a", "b", "tag:<t>", or an entity id.
    pub fn targets(&self, target: &str, ctx: Ctx) -> Vec<Uid> {
        match target {
            "self" | "a" => ctx.a.into_iter().collect(),
            "b" | "other" => ctx.b.into_iter().collect(),
            t if t.starts_with("tag:") => self.with_tag(&t[4..]),
            id => self.entities.iter().filter(|e| e.alive && e.id.as_deref() == Some(id)).map(|e| e.uid).collect(),
        }
    }

    pub fn var(&self, name: &str, on: Option<Uid>) -> Value {
        match on {
            Some(uid) => self.get(uid).and_then(|e| e.vars.get(name).cloned()).unwrap_or(Value::Null),
            None => self.vars.get(name).cloned().unwrap_or(Value::Null),
        }
    }

    pub fn set_var(&mut self, name: &str, value: Value, on: Option<Uid>) {
        match on {
            Some(uid) => {
                if let Some(e) = self.get_mut(uid) {
                    e.vars.insert(name.to_string(), value);
                }
            }
            None => {
                self.vars.insert(name.to_string(), value);
            }
        }
    }

    // ---------- Input ----------

    pub fn key(&self, k: &str) -> bool {
        self.input.keys.contains(k)
    }
    pub fn pressed(&self, k: &str) -> bool {
        self.input.keys.contains(k) && !self.prev_keys.contains(k)
    }
    pub fn released(&self, k: &str) -> bool {
        !self.input.keys.contains(k) && self.prev_keys.contains(k)
    }
    pub fn tapped(&self) -> bool {
        self.input.pointer_down && !self.prev_pointer_down
    }
    /// Pointer in world coordinates.
    pub fn pointer_world(&self) -> Option<[f32; 2]> {
        let z = self.zoom();
        self.input.pointer.map(|p| [p[0] / z + self.camera[0], p[1] / z + self.camera[1]])
    }
    fn zoom(&self) -> f32 {
        self.scene.camera.zoom.unwrap_or(1.0).max(0.1)
    }
    pub fn end_input(&mut self) {
        self.prev_keys = self.input.keys.clone();
        self.prev_pointer_down = self.input.pointer_down;
    }

    // ---------- Behaviours and physics ----------

    pub fn behaviours(&mut self) {
        let (left, right) = (self.key("left") || self.key("a"), self.key("right") || self.key("d"));
        let (up, down) = (self.key("up") || self.key("w"), self.key("down") || self.key("s"));
        let jump = self.pressed("up") || self.pressed("w") || self.pressed("space");
        let pointer = self.pointer_world();
        let b = self.bounds();
        let centers: Vec<(Uid, Vec<String>, [f32; 2])> = self.entities.iter().filter(|e| e.alive).map(|e| (e.uid, e.tags.clone(), e.center())).collect();
        for e in self.entities.iter_mut().filter(|e| e.alive) {
            e.age += DT;
            for beh in e.behaviours.clone() {
                match beh {
                    Behaviour::Platformer { speed, jump: j } => {
                        e.vel[0] = (right as i32 - left as i32) as f32 * speed;
                        if e.vel[0] != 0.0 {
                            e.flip = e.vel[0] < 0.0;
                        }
                        if jump && e.on_ground {
                            e.vel[1] = -j;
                            e.on_ground = false;
                        }
                    }
                    Behaviour::Topdown { speed } => {
                        let (dx, dy) = ((right as i32 - left as i32) as f32, (down as i32 - up as i32) as f32);
                        let len = (dx * dx + dy * dy).sqrt().max(1.0);
                        e.vel = [dx / len * speed, dy / len * speed];
                        if dx != 0.0 {
                            e.flip = dx < 0.0;
                        }
                    }
                    Behaviour::Patrol { speed, range, vertical } => {
                        let axis = vertical as usize;
                        if (e.pos[axis] - e.origin[axis]).abs() >= range && (e.pos[axis] - e.origin[axis]).signum() == e.dir {
                            e.dir = -e.dir;
                        }
                        e.vel[axis] = e.dir * speed;
                        if !vertical {
                            e.flip = e.dir < 0.0;
                        }
                    }
                    Behaviour::Follow { target, speed } => {
                        let me = e.center();
                        if let Some((_, _, c)) = centers.iter().find(|(uid, tags, _)| *uid != e.uid && tags.contains(&target)) {
                            let (dx, dy) = (c[0] - me[0], c[1] - me[1]);
                            let len = (dx * dx + dy * dy).sqrt();
                            if len > 1.0 {
                                e.vel = [dx / len * speed, dy / len * speed];
                            } else {
                                e.vel = [0.0, 0.0];
                            }
                        }
                    }
                    Behaviour::Bounce => {
                        if (e.pos[0] < b[0] && e.vel[0] < 0.0) || (e.pos[0] + e.size[0] > b[0] + b[2] && e.vel[0] > 0.0) {
                            e.vel[0] = -e.vel[0];
                        }
                        if (e.pos[1] < b[1] && e.vel[1] < 0.0) || (e.pos[1] + e.size[1] > b[1] + b[3] && e.vel[1] > 0.0) {
                            e.vel[1] = -e.vel[1];
                        }
                    }
                    Behaviour::Wrap => {
                        if e.pos[0] + e.size[0] < b[0] {
                            e.pos[0] = b[0] + b[2];
                        } else if e.pos[0] > b[0] + b[2] {
                            e.pos[0] = b[0] - e.size[0];
                        }
                        if e.pos[1] + e.size[1] < b[1] {
                            e.pos[1] = b[1] + b[3];
                        } else if e.pos[1] > b[1] + b[3] {
                            e.pos[1] = b[1] - e.size[1];
                        }
                    }
                    Behaviour::Spin { speed } => e.rotation = (e.rotation + speed * DT) % 360.0,
                    Behaviour::Lifetime { seconds } => {
                        if e.age >= seconds {
                            e.alive = false;
                        }
                    }
                    Behaviour::Offscreen => {
                        let m = 64.0;
                        if e.pos[0] + e.size[0] < b[0] - m || e.pos[0] > b[0] + b[2] + m || e.pos[1] + e.size[1] < b[1] - m || e.pos[1] > b[1] + b[3] + m {
                            e.alive = false;
                        }
                    }
                    Behaviour::Pointer { axis } => {
                        if let Some(p) = pointer {
                            if axis.as_deref() != Some("y") {
                                e.pos[0] = p[0] - e.size[0] / 2.0;
                            }
                            if axis.as_deref() != Some("x") {
                                e.pos[1] = p[1] - e.size[1] / 2.0;
                            }
                        }
                    }
                }
            }
        }
    }

    pub fn physics(&mut self) {
        let g = self.scene.gravity;
        let solids: Vec<(Uid, [f32; 2], [f32; 2])> = self.entities.iter().filter(|e| e.alive && e.solid()).map(|e| (e.uid, e.pos, e.size)).collect();
        for e in self.entities.iter_mut().filter(|e| e.alive) {
            match e.body {
                BodyKind::Dynamic => {
                    e.vel[0] += g[0] * DT;
                    e.vel[1] = (e.vel[1] + g[1] * DT).clamp(-4000.0, 4000.0);
                    if e.sensor {
                        e.pos[0] += e.vel[0] * DT;
                        e.pos[1] += e.vel[1] * DT;
                        continue;
                    }
                    e.on_ground = false;
                    for axis in 0..2 {
                        e.pos[axis] += e.vel[axis] * DT;
                        for (uid, p, s) in &solids {
                            if *uid == e.uid {
                                continue;
                            }
                            let hit = e.pos[0] < p[0] + s[0] && p[0] < e.pos[0] + e.size[0] && e.pos[1] < p[1] + s[1] && p[1] < e.pos[1] + e.size[1];
                            if !hit {
                                continue;
                            }
                            if e.vel[axis] > 0.0 {
                                e.pos[axis] = p[axis] - e.size[axis];
                                if axis == 1 {
                                    e.on_ground = true;
                                }
                            } else if e.vel[axis] < 0.0 {
                                e.pos[axis] = p[axis] + s[axis];
                            } else {
                                // Resting inside (e.g. spawned overlapping): push out upward.
                                if axis == 1 {
                                    e.pos[1] = p[1] - e.size[1];
                                    e.on_ground = true;
                                }
                            }
                            if axis == 0 {
                                // Walls turn patrollers around.
                                if e.behaviours.iter().any(|b| matches!(b, Behaviour::Patrol { vertical: false, .. })) {
                                    e.dir = -e.dir;
                                }
                            }
                            e.vel[axis] = 0.0;
                        }
                    }
                }
                BodyKind::Kinematic | BodyKind::None => {
                    e.pos[0] += e.vel[0] * DT;
                    e.pos[1] += e.vel[1] * DT;
                }
                BodyKind::Static => {}
            }
        }
    }

    /// Pairs touching now that weren't last frame.
    pub fn new_contacts(&mut self) -> Vec<(Uid, Uid)> {
        let tags = &self.collide_tags;
        let live: Vec<&Entity> = self.entities.iter().filter(|e| e.alive && !e.hud && (e.body != BodyKind::None || e.sensor || e.tags.iter().any(|t| tags.contains(t)))).collect();
        let mut now = HashSet::new();
        for i in 0..live.len() {
            for j in i + 1..live.len() {
                let (a, b) = (live[i], live[j]);
                if a.body == BodyKind::Static && b.body == BodyKind::Static {
                    continue;
                }
                if overlap(a, b, 0.5) {
                    now.insert((a.uid.min(b.uid), a.uid.max(b.uid)));
                }
            }
        }
        let fresh: Vec<(Uid, Uid)> = now.difference(&self.touching).cloned().collect();
        self.touching = now;
        let mut fresh = fresh;
        fresh.sort();
        self.contacts = fresh.iter().map(|(a, b)| [*a, *b]).collect();
        fresh
    }

    pub(crate) fn every_mut(&mut self) -> &mut Vec<f32> {
        &mut self.every
    }

    /// Remember whether rule `i`'s variable condition holds now; return what it was.
    pub(crate) fn var_state(&mut self, i: usize, now: bool) -> bool {
        match self.var_was.get_mut(i) {
            Some(was) => std::mem::replace(was, now),
            None => false,
        }
    }

    pub fn shake(&mut self, strength: f32, seconds: f32) {
        self.shake = (strength, seconds);
    }

    pub fn update_camera(&mut self) {
        let b = self.bounds();
        let z = self.zoom();
        let (vw, vh) = (self.game.size[0] / z, self.game.size[1] / z);
        if let Some(tag) = self.scene.camera.follow.clone() {
            if let Some(e) = self.entities.iter().find(|e| e.alive && e.has(&tag)) {
                let c = e.center();
                self.camera = [c[0] - vw / 2.0, c[1] - vh / 2.0];
            }
        }
        self.camera[0] = self.camera[0].clamp(b[0], (b[0] + b[2] - vw).max(b[0]));
        self.camera[1] = self.camera[1].clamp(b[1], (b[1] + b[3] - vh).max(b[1]));
        for e in self.entities.iter_mut() {
            e.anim_time += DT;
        }
        if self.shake.1 > 0.0 {
            self.shake.1 -= DT;
        }
    }

    pub fn sweep(&mut self) {
        self.entities.retain(|e| e.alive);
    }

    // ---------- Drawing ----------

    pub fn frame(&mut self, take_logs: bool) -> Frame {
        let z = self.zoom();
        let (mut sx, mut sy) = (0.0, 0.0);
        if self.shake.1 > 0.0 {
            sx = (self.random() as f32 * 2.0 - 1.0) * self.shake.0;
            sy = (self.random() as f32 * 2.0 - 1.0) * self.shake.0;
        }
        let mut order: Vec<&Entity> = self.entities.iter().filter(|e| e.alive && e.visible).collect();
        order.sort_by_key(|e| (e.hud, e.layer, e.uid));
        let mut draws = Vec::new();
        let images = &self.game.assets.images;
        for e in order {
            let (x, y, w, h) = if e.hud {
                (e.pos[0], e.pos[1], e.size[0], e.size[1])
            } else {
                ((e.pos[0] - self.camera[0]) * z + sx, (e.pos[1] - self.camera[1]) * z + sy, e.size[0] * z, e.size[1] * z)
            };
            // Off screen: skip.
            if x > self.game.size[0] + 8.0 || y > self.game.size[1] + 8.0 || x + w < -8.0 || y + h < -8.0 {
                continue;
            }
            if let Some(s) = &e.sprite {
                let asset = images.get(&s.image);
                let (fw, fh) = asset.and_then(|a| a.frame).map(|f| (f[0], f[1])).unwrap_or((0.0, 0.0));
                let f = match (&s.anim, asset) {
                    (Some(name), Some(a)) => match a.anims.get(name) {
                        Some(an) if !an.frames.is_empty() => {
                            let i = (e.anim_time * an.fps) as usize;
                            an.frames[if an.looped { i % an.frames.len() } else { i.min(an.frames.len() - 1) }]
                        }
                        _ => s.frame,
                    },
                    _ => s.frame,
                };
                if e.has("tile") && fw > 0.0 && w > fw * z * 1.5 {
                    // A merged run of image tiles isn't made (image tiles aren't merged); draw once.
                }
                draws.push(Draw::Image { img: s.image.clone(), f, fw, fh, x, y, w, h, a: e.opacity, rot: e.rotation, flip: e.flip });
            } else if let Some(c) = &e.color {
                if e.circle {
                    draws.push(Draw::Circle { x, y, w, h, c: c.clone(), a: e.opacity });
                } else {
                    draws.push(Draw::Rect { x, y, w, h, c: c.clone(), a: e.opacity, rot: e.rotation });
                }
            }
            if let Some(t) = &e.text {
                let s = interpolate(&t.value, &self.vars, &e.vars);
                draws.push(Draw::Text { s, x, y, w, size: t.size * if e.hud { 1.0 } else { z }, c: t.color.clone().unwrap_or("#ffffff".into()), align: t.align.clone().unwrap_or("center".into()), a: e.opacity });
            }
        }
        let calls = std::mem::take(&mut self.host_calls);
        Frame {
            frame: self.frame,
            time: self.time,
            background: self.scene.background.clone().unwrap_or(self.game.background.clone()),
            draws,
            sounds: std::mem::take(&mut self.sounds),
            status: self.status.clone(),
            message: self.message.clone(),
            vars: self.vars.clone(),
            calls,
            contacts: self.contacts.clone(),
            logs: if take_logs { std::mem::take(&mut self.logs) } else { Vec::new() },
            errors: if take_logs { std::mem::take(&mut self.errors) } else { Vec::new() },
        }
    }

    // ---------- Rules ----------

    pub fn condition(&mut self, c: &Condition, ctx: Ctx) -> bool {
        match c {
            Condition::Var { var, op, value } => {
                let (name, on) = match var.split_once('.') {
                    Some(("self", n)) => (n, ctx.a),
                    Some(("other", n)) | Some(("b", n)) => (n, ctx.b),
                    _ => (var.as_str(), None),
                };
                compare(&self.var(name, on), op, value)
            }
            Condition::Count { count, op, value } => compare(&json!(self.with_tag(count).len()), op, &json!(value)),
            Condition::Chance { chance } => self.random() < *chance,
        }
    }

    /// Apply an action. Returns a script function to call (outside any borrow), if any.
    pub fn act(&mut self, a: &Action, ctx: Ctx) -> Option<String> {
        match a {
            Action::Set { var, value, on } => {
                let targets = on.as_ref().map(|t| self.targets(t, ctx));
                match targets {
                    Some(list) => list.into_iter().for_each(|u| self.set_var(var, value.clone(), Some(u))),
                    None => self.set_var(var, value.clone(), None),
                }
            }
            Action::Add { var, value, on } => {
                let list: Vec<Option<Uid>> = match on {
                    Some(t) => self.targets(t, ctx).into_iter().map(Some).collect(),
                    None => vec![None],
                };
                for u in list {
                    let now = num(&self.var(var, u)) + value;
                    let v = if now.fract() == 0.0 && now.abs() < 9e15 { json!(now as i64) } else { json!(now) };
                    self.set_var(var, v, u);
                }
            }
            Action::Spawn { prefab, at, pos, offset, vel } => {
                let off = offset.unwrap_or([0.0, 0.0]);
                let places: Vec<[f32; 2]> = match (at, pos) {
                    (Some(t), _) => self.targets(t, ctx).into_iter().filter_map(|u| self.get(u).map(|e| e.center())).collect(),
                    (None, Some(p)) => vec![*p],
                    (None, None) => vec![[0.0, 0.0]],
                };
                let size = self.game.prefabs.get(prefab).and_then(|p| p.size).unwrap_or([16.0, 16.0]);
                for p in places {
                    let centered = if at.is_some() { [p[0] - size[0] / 2.0 + off[0], p[1] - size[1] / 2.0 + off[1]] } else { [p[0] + off[0], p[1] + off[1]] };
                    let def = EntityDef { prefab: Some(prefab.clone()), ..Default::default() };
                    if let Some(uid) = self.spawn_def(&def, Some(centered)) {
                        if let (Some(v), Some(e)) = (vel, self.get_mut(uid)) {
                            e.vel = *v;
                        }
                    }
                }
            }
            Action::Destroy { target } => {
                for u in self.targets(target, ctx) {
                    if let Some(e) = self.get_mut(u) {
                        e.alive = false;
                    }
                }
            }
            Action::Move { target, to, by } => {
                for u in self.targets(target, ctx) {
                    if let Some(e) = self.get_mut(u) {
                        if let Some(t) = to {
                            e.pos = *t;
                        }
                        if let Some(d) = by {
                            e.pos = [e.pos[0] + d[0], e.pos[1] + d[1]];
                        }
                    }
                }
            }
            Action::Velocity { target, vel } => {
                for u in self.targets(target, ctx) {
                    if let Some(e) = self.get_mut(u) {
                        e.vel = *vel;
                    }
                }
            }
            Action::Change { target, visible, color, text, anim } => {
                for u in self.targets(target, ctx) {
                    if let Some(e) = self.get_mut(u) {
                        if let Some(v) = visible {
                            e.visible = *v;
                        }
                        if let Some(c) = color {
                            e.color = Some(c.clone());
                        }
                        if let Some(t) = text {
                            match &mut e.text {
                                Some(x) => x.value = t.clone(),
                                None => e.text = Some(Text { value: t.clone(), size: 16.0, color: None, align: None }),
                            }
                        }
                        if let (Some(a), Some(s)) = (anim, &mut e.sprite) {
                            if s.anim.as_ref() != Some(a) {
                                s.anim = Some(a.clone());
                                e.anim_time = 0.0;
                            }
                        }
                    }
                }
            }
            Action::Sound { sound } => self.sounds.push(sound.clone()),
            Action::Shake { strength, seconds } => self.shake(*strength, *seconds),
            Action::Scene { scene } => self.pending_scene = Some(scene.clone()),
            Action::Restart => self.pending_restart = true,
            Action::Win { message } => {
                self.status = Status::Won;
                self.message = Some(interpolate(message.as_deref().unwrap_or("You win!"), &self.vars, &BTreeMap::new()));
            }
            Action::Lose { message } => {
                self.status = Status::Lost;
                self.message = Some(interpolate(message.as_deref().unwrap_or("Game over"), &self.vars, &BTreeMap::new()));
            }
            Action::Call { function } => return Some(function.clone()),
        }
        None
    }
}

/// "Score: {score}" → "Score: 12"; "{self.hp}" reads the entity's own variable.
pub fn interpolate(text: &str, vars: &BTreeMap<String, Value>, own: &BTreeMap<String, Value>) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(i) = rest.find('{') {
        out.push_str(&rest[..i]);
        let Some(j) = rest[i..].find('}') else {
            out.push_str(&rest[i..]);
            return out;
        };
        let key = &rest[i + 1..i + j];
        let v = match key.strip_prefix("self.") {
            Some(k) => own.get(k),
            None => vars.get(key),
        };
        match v {
            Some(Value::String(s)) => out.push_str(s),
            Some(Value::Number(n)) => out.push_str(&n.to_string()),
            Some(other) if !other.is_null() => out.push_str(&other.to_string()),
            _ => out.push_str(&rest[i..i + j + 1]),
        }
        rest = &rest[i + j + 1..];
    }
    out.push_str(rest);
    out
}
