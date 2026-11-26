//! Rhai scripts, run inside the engine (so they work everywhere: browser, server checks, tests).
//!
//! A script may define `on_start()`, `on_tick(dt)`, `on_collide(a, b)` and any function a rule
//! calls (`{ "action": "call", "function": "hurt" }` → `fn hurt(a, b)`). Entities are numbers (ids).
//! Each call is limited in operations, call depth and sizes, so a script can't hang the game.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use rhai::{Array, Dynamic, Engine as Rhai, EvalAltResult, Scope, AST};
use serde_json::{json, Value};

use crate::format::{EntityDef, Text};
use crate::world::{Uid, World};

pub struct Scripts {
    rhai: Rhai,
    ast: AST,
    scope: Scope<'static>,
    functions: BTreeMap<String, usize>,
}

fn to_dynamic(v: &Value) -> Dynamic {
    match v {
        Value::Null => Dynamic::UNIT,
        Value::Bool(b) => (*b).into(),
        Value::Number(n) => match n.as_i64() {
            Some(i) => i.into(),
            None => n.as_f64().unwrap_or(0.0).into(),
        },
        Value::String(s) => s.clone().into(),
        Value::Array(a) => Dynamic::from_array(a.iter().map(to_dynamic).collect()),
        Value::Object(_) => v.to_string().into(),
    }
}

fn to_value(d: &Dynamic) -> Value {
    if d.is_unit() {
        Value::Null
    } else if let Ok(b) = d.as_bool() {
        json!(b)
    } else if let Ok(i) = d.as_int() {
        json!(i)
    } else if let Ok(f) = d.as_float() {
        json!(f)
    } else if d.is_string() {
        json!(d.clone().into_string().unwrap_or_default())
    } else if d.is_array() {
        Value::Array(d.clone().into_array().unwrap_or_default().iter().map(to_value).collect())
    } else {
        json!(d.to_string())
    }
}

impl Scripts {
    /// Compile the game's `.rhai` scripts (joined) with the engine API bound to `world`.
    pub fn new(world: Rc<RefCell<World>>, sources: &[(&str, &str)]) -> Result<Scripts, String> {
        let mut rhai = Rhai::new();
        rhai.set_max_operations(2_000_000);
        rhai.set_max_call_levels(32);
        rhai.set_max_expr_depths(64, 64);
        rhai.set_max_string_size(100_000);
        rhai.set_max_array_size(10_000);
        rhai.set_max_map_size(10_000);
        register(&mut rhai, world);
        let mut ast = AST::empty();
        for (name, src) in sources {
            let part = rhai.compile(*src).map_err(|e| format!("{name}: {e}"))?;
            ast = ast.merge(&part);
        }
        let functions = ast.iter_functions().map(|f| (f.name.to_string(), f.params.len())).collect();
        let mut scope = Scope::new();
        // Top-level statements run once (constants, setup).
        rhai.run_ast_with_scope(&mut scope, &ast).map_err(|e| format!("script: {e}"))?;
        Ok(Scripts { rhai, ast, scope, functions })
    }

    pub fn has(&self, name: &str) -> bool {
        self.functions.contains_key(name)
    }

    /// Call `name` with as many of `args` as it takes.
    pub fn call(&mut self, name: &str, args: Vec<Dynamic>) -> Result<(), String> {
        let Some(&n) = self.functions.get(name) else { return Ok(()) };
        let args: Vec<Dynamic> = args.into_iter().take(n).chain(std::iter::repeat(Dynamic::UNIT)).take(n).collect();
        self.rhai.call_fn::<Dynamic>(&mut self.scope, &self.ast, name, args).map(|_| ()).map_err(|e: Box<EvalAltResult>| format!("{name}: {e}"))
    }
}

/** Any number (int or float) as f64, so scripts needn't care which they pass. */
fn n(d: Dynamic) -> f64 {
    d.as_float().or_else(|_| d.as_int().map(|i| i as f64)).unwrap_or(0.0)
}

pub fn uid_arg(u: Option<Uid>) -> Dynamic {
    u.map(|x| Dynamic::from(x as i64)).unwrap_or(Dynamic::from(-1_i64))
}

fn register(r: &mut Rhai, world: Rc<RefCell<World>>) {
    macro_rules! w {
        () => {
            world.clone()
        };
    }
    let id = |x: i64| x.max(0) as Uid;

    let wd = w!();
    // `spawn` is reserved in Rhai.
    r.register_fn("create", move |prefab: &str, x: Dynamic, y: Dynamic| -> i64 {
        let (x, y) = (n(x), n(y));
        let def = EntityDef { prefab: Some(prefab.to_string()), ..Default::default() };
        wd.borrow_mut().spawn_def(&def, Some([x as f32, y as f32])).map(|u| u as i64).unwrap_or(-1)
    });
    let wd = w!();
    r.register_fn("destroy", move |e: i64| {
        if let Some(x) = wd.borrow_mut().get_mut(id(e)) {
            x.alive = false;
        }
    });
    let wd = w!();
    r.register_fn("exists", move |e: i64| wd.borrow().get(id(e)).is_some());
    let wd = w!();
    r.register_fn("x", move |e: i64| wd.borrow().get(id(e)).map(|x| x.pos[0] as f64).unwrap_or(0.0));
    let wd = w!();
    r.register_fn("y", move |e: i64| wd.borrow().get(id(e)).map(|x| x.pos[1] as f64).unwrap_or(0.0));
    let wd = w!();
    r.register_fn("vx", move |e: i64| wd.borrow().get(id(e)).map(|x| x.vel[0] as f64).unwrap_or(0.0));
    let wd = w!();
    r.register_fn("vy", move |e: i64| wd.borrow().get(id(e)).map(|x| x.vel[1] as f64).unwrap_or(0.0));
    let wd = w!();
    r.register_fn("on_ground", move |e: i64| wd.borrow().get(id(e)).is_some_and(|x| x.on_ground));
    let wd = w!();
    r.register_fn("set_pos", move |e: i64, x: Dynamic, y: Dynamic| {
        let (x, y) = (n(x), n(y));
        if let Some(en) = wd.borrow_mut().get_mut(id(e)) {
            en.pos = [x as f32, y as f32];
        }
    });
    let wd = w!();
    r.register_fn("set_vel", move |e: i64, x: Dynamic, y: Dynamic| {
        let (x, y) = (n(x), n(y));
        if let Some(en) = wd.borrow_mut().get_mut(id(e)) {
            en.vel = [x as f32, y as f32];
        }
    });
    let wd = w!();
    r.register_fn("find", move |tag: &str| -> Array { wd.borrow().with_tag(tag).into_iter().map(|u| Dynamic::from(u as i64)).collect() });
    let wd = w!();
    r.register_fn("first", move |tag: &str| -> i64 { wd.borrow().with_tag(tag).first().map(|u| *u as i64).unwrap_or(-1) });
    let wd = w!();
    r.register_fn("count", move |tag: &str| -> i64 { wd.borrow().with_tag(tag).len() as i64 });
    let wd = w!();
    r.register_fn("has_tag", move |e: i64, tag: &str| wd.borrow().get(id(e)).is_some_and(|x| x.has(tag)));
    let wd = w!();
    r.register_fn("get", move |name: &str| to_dynamic(&wd.borrow().var(name, None)));
    let wd = w!();
    r.register_fn("set", move |name: &str, v: Dynamic| wd.borrow_mut().set_var(name, to_value(&v), None));
    let wd = w!();
    r.register_fn("get", move |e: i64, name: &str| to_dynamic(&wd.borrow().var(name, Some(id(e)))));
    let wd = w!();
    r.register_fn("set", move |e: i64, name: &str, v: Dynamic| wd.borrow_mut().set_var(name, to_value(&v), Some(id(e))));
    let wd = w!();
    r.register_fn("key", move |k: &str| wd.borrow().key(k));
    let wd = w!();
    r.register_fn("pressed", move |k: &str| wd.borrow().pressed(k));
    let wd = w!();
    r.register_fn("pointer_x", move || wd.borrow().pointer_world().map(|p| p[0] as f64).unwrap_or(-1.0));
    let wd = w!();
    r.register_fn("pointer_y", move || wd.borrow().pointer_world().map(|p| p[1] as f64).unwrap_or(-1.0));
    let wd = w!();
    r.register_fn("pointer_down", move || wd.borrow().input.pointer_down);
    let wd = w!();
    r.register_fn("random", move || wd.borrow_mut().random());
    let wd = w!();
    r.register_fn("random_int", move |a: i64, b: i64| -> i64 {
        let x = wd.borrow_mut().random();
        a + ((b - a + 1) as f64 * x).floor() as i64
    });
    let wd = w!();
    r.register_fn("time", move || wd.borrow().time);
    let wd = w!();
    r.register_fn("sound", move |s: &str| wd.borrow_mut().sounds.push(s.to_string()));
    let wd = w!();
    r.register_fn("shake", move |s: Dynamic, t: Dynamic| wd.borrow_mut().shake(n(s) as f32, n(t) as f32));
    let wd = w!();
    r.register_fn("scene", move |s: &str| wd.borrow_mut().pending_scene = Some(s.to_string()));
    let wd = w!();
    r.register_fn("restart", move || wd.borrow_mut().pending_restart = true);
    let wd = w!();
    r.register_fn("win", move |m: &str| {
        let mut w = wd.borrow_mut();
        w.status = crate::world::Status::Won;
        w.message = Some(m.to_string());
    });
    let wd = w!();
    r.register_fn("lose", move |m: &str| {
        let mut w = wd.borrow_mut();
        w.status = crate::world::Status::Lost;
        w.message = Some(m.to_string());
    });
    let wd = w!();
    r.register_fn("text", move |e: i64, s: &str| {
        if let Some(en) = wd.borrow_mut().get_mut(id(e)) {
            match &mut en.text {
                Some(t) => t.value = s.to_string(),
                None => en.text = Some(Text { value: s.to_string(), size: 16.0, color: None, align: None }),
            }
        }
    });
    let wd = w!();
    r.register_fn("color", move |e: i64, c: &str| {
        if let Some(en) = wd.borrow_mut().get_mut(id(e)) {
            en.color = Some(c.to_string());
        }
    });
    let wd = w!();
    r.register_fn("visible", move |e: i64, v: bool| {
        if let Some(en) = wd.borrow_mut().get_mut(id(e)) {
            en.visible = v;
        }
    });
    let wd = w!();
    r.register_fn("anim", move |e: i64, a: &str| {
        if let Some(en) = wd.borrow_mut().get_mut(id(e)) {
            if let Some(s) = &mut en.sprite {
                if s.anim.as_deref() != Some(a) {
                    s.anim = Some(a.to_string());
                    en.anim_time = 0.0;
                }
            }
        }
    });
    let wd = w!();
    r.register_fn("log", move |m: Dynamic| wd.borrow_mut().logs.push(m.to_string()));
    let wd = w!();
    r.on_print(move |m| wd.borrow_mut().logs.push(m.to_string()));
}
