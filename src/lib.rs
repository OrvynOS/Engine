//! Orvyn Engine: 2D games that humans and agents make and publish on Orvyn.
//!
//! A game is one JSON file (`format.rs`): scenes of entities, prefabs, tilemaps, ready-made
//! behaviours, no-code rules, and optional scripts (Rhai, run here; TypeScript/JavaScript, run by the
//! browser player through the same API). The engine steps at a fixed 60 Hz from the players' input
//! and returns a frame: what to draw, sounds to play, and the game's state. It doesn't draw itself,
//! so the same game runs in the browser (WebAssembly), in tests and on servers (checks, thumbnails).

pub mod format;
pub mod playtest;
pub mod render;
pub mod replay;
pub mod script;
pub mod validate;
pub mod world;
#[cfg(target_arch = "wasm32")]
mod wasm;

use std::cell::RefCell;
use std::rc::Rc;

use serde_json::{json, Value};

use format::{Event, Game, FORMAT};
use script::{uid_arg, Scripts};
use world::{Ctx, Frame, Input, Status, Uid, World, DT};

pub struct Engine {
    world: Rc<RefCell<World>>,
    scripts: Option<Scripts>,
    started: bool,
    pub halted: Option<String>,
}

impl Engine {
    /// Load a game from its JSON. Errors are the problems found (see `validate`).
    pub fn load(json: &str) -> Result<Engine, Vec<String>> {
        let game: Game = serde_json::from_str(json).map_err(|e| vec![format!("the game file isn't valid: {e}")])?;
        Self::from_game(game)
    }

    pub fn from_game(game: Game) -> Result<Engine, Vec<String>> {
        let problems = validate::problems(&game);
        if !problems.is_empty() {
            return Err(problems);
        }
        let sources: Vec<(String, String)> = game.scripts.iter().filter(|(k, _)| k.ends_with(".rhai")).map(|(k, v)| (k.clone(), v.clone())).collect();
        let world = Rc::new(RefCell::new(World::new(game)));
        let scripts = if sources.is_empty() {
            None
        } else {
            let refs: Vec<(&str, &str)> = sources.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
            Some(Scripts::new(world.clone(), &refs).map_err(|e| vec![e])?)
        };
        Ok(Engine { world, scripts, started: false, halted: None })
    }

    pub fn status(&self) -> Status {
        self.world.borrow().status.clone()
    }

    pub fn vars(&self) -> serde_json::Map<String, Value> {
        self.world.borrow().vars.clone().into_iter().collect()
    }

    pub fn world(&self) -> std::cell::Ref<'_, World> {
        self.world.borrow()
    }

    fn script(&mut self, name: &str, args: Vec<rhai::Dynamic>) {
        if let Some(s) = &mut self.scripts {
            if let Err(e) = s.call(name, args) {
                // A broken script stops the game with its error (shown by the player).
                self.halted = Some(e.clone());
                self.world.borrow_mut().errors.push(e);
            }
        }
    }

    /// Run a rule's actions; `call` actions go to Rhai (if it has the function) or to the player.
    fn run_actions(&mut self, actions: &[format::Action], ctx: Ctx) {
        for a in actions {
            let call = self.world.borrow_mut().act(a, ctx);
            if let Some(f) = call {
                if self.scripts.as_ref().is_some_and(|s| s.has(&f)) {
                    self.script(&f, vec![uid_arg(ctx.a), uid_arg(ctx.b)]);
                } else {
                    self.world.borrow_mut().host_calls.push(world::HostCall { function: f, a: ctx.a, b: ctx.b });
                }
            }
        }
    }

    fn rules_for(&self, f: impl Fn(&Event) -> bool) -> Vec<(usize, format::Rule)> {
        self.world.borrow().scene.rules.iter().enumerate().filter(|(_, r)| f(&r.when)).map(|(i, r)| (i, r.clone())).collect()
    }

    fn fire(&mut self, rules: Vec<(usize, format::Rule)>, ctx: Ctx) {
        for (_, rule) in rules {
            let ok = rule.conditions.iter().all(|c| self.world.borrow_mut().condition(c, ctx));
            if ok {
                self.run_actions(&rule.actions, ctx);
            }
        }
    }

    fn on_scene_start(&mut self) {
        let start = self.rules_for(|e| matches!(e, Event::Start));
        self.fire(start, Ctx::default());
        self.script("on_start", vec![]);
    }

    /// Advance one frame (1/60 s) with this input, and return what to draw.
    pub fn step(&mut self, input: Input) -> Frame {
        self.world.borrow_mut().input = input;
        if self.world.borrow().status != Status::Running || self.halted.is_some() {
            let mut w = self.world.borrow_mut();
            w.end_input();
            return w.frame(true);
        }
        if !self.started {
            self.started = true;
            self.on_scene_start();
        }
        {
            let mut w = self.world.borrow_mut();
            w.behaviours();
            w.physics();
        }

        // Contacts: collide rules (both orders of the tag pair), then scripts.
        let fresh = self.world.borrow_mut().new_contacts();
        let collide_rules = self.rules_for(|e| matches!(e, Event::Collide { .. }));
        for (x, y) in &fresh {
            for (_, rule) in &collide_rules {
                let Event::Collide { a, b } = &rule.when else { continue };
                let pair = {
                    let w = self.world.borrow();
                    match (w.get(*x), w.get(*y)) {
                        (Some(ex), Some(ey)) if ex.has(a) && ey.has(b) => Some(Ctx { a: Some(*x), b: Some(*y) }),
                        (Some(ex), Some(ey)) if ey.has(a) && ex.has(b) => Some(Ctx { a: Some(*y), b: Some(*x) }),
                        _ => None,
                    }
                };
                if let Some(ctx) = pair {
                    self.fire(vec![(0, rule.clone())], ctx);
                }
            }
            if self.scripts.as_ref().is_some_and(|s| s.has("on_collide")) {
                self.script("on_collide", vec![rhai::Dynamic::from(*x as i64), rhai::Dynamic::from(*y as i64)]);
            }
        }

        // Ticks, timers, keys, taps, variables.
        let tick = self.rules_for(|e| matches!(e, Event::Tick));
        self.fire(tick, Ctx::default());
        for (i, rule) in self.rules_for(|e| matches!(e, Event::Every { .. })) {
            let Event::Every { seconds } = rule.when else { continue };
            let due = {
                let mut w = self.world.borrow_mut();
                w_every(&mut w, i, seconds)
            };
            if due {
                self.fire(vec![(i, rule)], Ctx::default());
            }
        }
        for (i, rule) in self.rules_for(|e| matches!(e, Event::Key { .. })) {
            let Event::Key { key, state } = &rule.when else { continue };
            let hit = {
                let w = self.world.borrow();
                match state.as_str() {
                    "down" => w.key(key),
                    "released" => w.released(key),
                    _ => w.pressed(key),
                }
            };
            if hit {
                self.fire(vec![(i, rule)], Ctx::default());
            }
        }
        let tapped = self.world.borrow().tapped();
        if tapped {
            for (i, rule) in self.rules_for(|e| matches!(e, Event::Tap { .. })) {
                let Event::Tap { tag } = &rule.when else { continue };
                match tag {
                    None => self.fire(vec![(i, rule.clone())], Ctx::default()),
                    Some(t) => {
                        let hits: Vec<Uid> = {
                            let w = self.world.borrow();
                            let world_p = w.pointer_world();
                            let screen_p = w.input.pointer;
                            w.entities
                                .iter()
                                .filter(|e| e.alive && e.visible && e.has(t))
                                .filter(|e| {
                                    let p = if e.hud { screen_p } else { world_p };
                                    p.is_some_and(|p| p[0] >= e.pos[0] && p[0] <= e.pos[0] + e.size[0] && p[1] >= e.pos[1] && p[1] <= e.pos[1] + e.size[1])
                                })
                                .map(|e| e.uid)
                                .collect()
                        };
                        for u in hits {
                            self.fire(vec![(i, rule.clone())], Ctx { a: Some(u), b: None });
                        }
                    }
                }
            }
        }
        for (i, rule) in self.rules_for(|e| matches!(e, Event::Var { .. })) {
            let Event::Var { name, op, value } = &rule.when else { continue };
            let now = {
                let w = self.world.borrow();
                world::compare(&w.var(name, None), op, value)
            };
            let was = self.world.borrow_mut().var_state(i, now);
            if now && !was {
                self.fire(vec![(i, rule)], Ctx::default());
            }
        }
        self.script("on_tick", vec![rhai::Dynamic::from(DT as f64)]);

        // Scene changes and the end of the frame.
        let (next, restart) = {
            let mut w = self.world.borrow_mut();
            (w.pending_scene.take(), std::mem::take(&mut w.pending_restart))
        };
        if restart {
            let mut w = self.world.borrow_mut();
            let start = w.game.start.clone();
            w.vars = w.game.vars.clone();
            w.load_scene(&start);
            drop(w);
            self.on_scene_start();
        } else if let Some(scene) = next {
            self.world.borrow_mut().load_scene(&scene);
            self.on_scene_start();
        }
        let mut w = self.world.borrow_mut();
        w.sweep();
        w.update_camera();
        w.end_input();
        w.frame += 1;
        w.time += DT as f64;
        w.frame(true)
    }

    /// The API for the player's TypeScript/JavaScript scripts: `{ "op": "...", ... }` → a value.
    pub fn api(&mut self, cmd: &Value) -> Value {
        let op = cmd["op"].as_str().unwrap_or("");
        let id = cmd["id"].as_u64().map(|x| x as Uid);
        let f = |k: &str| cmd[k].as_f64().unwrap_or(0.0) as f32;
        let s = |k: &str| cmd[k].as_str().unwrap_or("").to_string();
        let mut w = self.world.borrow_mut();
        match op {
            "spawn" => {
                let def = format::EntityDef { prefab: Some(s("prefab")), ..Default::default() };
                json!(w.spawn_def(&def, Some([f("x"), f("y")])))
            }
            "destroy" => {
                if let Some(e) = id.and_then(|u| w.get_mut(u)) {
                    e.alive = false;
                }
                Value::Null
            }
            "get_entity" => match id.and_then(|u| w.get(u)) {
                Some(e) => json!({ "id": e.uid, "name": e.id, "tags": e.tags, "x": e.pos[0], "y": e.pos[1], "w": e.size[0], "h": e.size[1], "vx": e.vel[0], "vy": e.vel[1], "onGround": e.on_ground, "vars": e.vars }),
                None => Value::Null,
            },
            "set_pos" => {
                if let Some(e) = id.and_then(|u| w.get_mut(u)) {
                    e.pos = [f("x"), f("y")];
                }
                Value::Null
            }
            "set_vel" => {
                if let Some(e) = id.and_then(|u| w.get_mut(u)) {
                    e.vel = [f("x"), f("y")];
                }
                Value::Null
            }
            "find" => json!(w.with_tag(&s("tag"))),
            "get" => w.var(&s("name"), id),
            "set" => {
                w.set_var(&s("name"), cmd["value"].clone(), id);
                Value::Null
            }
            "random" => json!(w.random()),
            "sound" => {
                w.sounds.push(s("name"));
                Value::Null
            }
            "scene" => {
                w.pending_scene = Some(s("name"));
                Value::Null
            }
            "win" | "lose" => {
                w.status = if op == "win" { Status::Won } else { Status::Lost };
                w.message = Some(s("message"));
                Value::Null
            }
            "text" => {
                if let Some(e) = id.and_then(|u| w.get_mut(u)) {
                    match &mut e.text {
                        Some(t) => t.value = s("text"),
                        None => e.text = Some(format::Text { value: s("text"), size: 16.0, color: None, align: None }),
                    }
                }
                Value::Null
            }
            "key" => json!(w.key(&s("key"))),
            "pressed" => json!(w.pressed(&s("key"))),
            _ => json!({ "error": format!("unknown op {op}") }),
        }
    }
}

fn w_every(w: &mut World, i: usize, seconds: f32) -> bool {
    w.tick_every(i, seconds)
}

impl World {
    pub(crate) fn tick_every(&mut self, i: usize, seconds: f32) -> bool {
        let timers = self.every_mut();
        if i >= timers.len() {
            return false;
        }
        timers[i] += DT;
        if timers[i] + 1e-6 >= seconds.max(DT) {
            timers[i] -= seconds.max(DT);
            true
        } else {
            false
        }
    }
}

/// Run a game headless for `frames` frames with no input: the problems it hits (script errors,
/// missing prefabs or scenes at run time). Used before publishing.
pub fn smoke_test(json: &str, frames: u32) -> Result<Vec<String>, Vec<String>> {
    let mut e = Engine::load(json)?;
    let mut errors = Vec::new();
    for _ in 0..frames {
        let f = e.step(Input::default());
        errors.extend(f.errors);
        if e.halted.is_some() || f.status != Status::Running {
            break;
        }
    }
    errors.dedup();
    Ok(errors)
}

pub const GAME_FORMAT: &str = FORMAT;
