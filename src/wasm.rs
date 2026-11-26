//! The WebAssembly boundary: a few exported functions that take and return JSON in linear memory,
//! so the player needs no binding generator. The player:
//!   1. `orvyn_alloc(len)` → writes UTF-8 JSON there → `orvyn_load(ptr, len)` (0 = ok, 1 = problems)
//!   2. each frame: `orvyn_step(ptr, len)` with `{ keys: [...], pointer: [x, y] | null, down }`
//!   3. reads the result with `orvyn_out_ptr()` / `orvyn_out_len()` (JSON) and frees what it allocated.

use std::cell::RefCell;
use std::collections::HashSet;

use serde_json::{json, Value};

use crate::world::Input;
use crate::Engine;

thread_local! {
    static ENGINE: RefCell<Option<Engine>> = const { RefCell::new(None) };
    static OUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

fn set_out(v: &Value) {
    OUT.with(|o| *o.borrow_mut() = serde_json::to_vec(v).unwrap_or_default());
}

unsafe fn read(ptr: *const u8, len: usize) -> String {
    String::from_utf8_lossy(unsafe { std::slice::from_raw_parts(ptr, len) }).into_owned()
}

#[unsafe(no_mangle)]
pub extern "C" fn orvyn_alloc(len: usize) -> *mut u8 {
    let mut v = Vec::<u8>::with_capacity(len.max(1));
    let p = v.as_mut_ptr();
    std::mem::forget(v);
    p
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn orvyn_free(ptr: *mut u8, len: usize) {
    drop(unsafe { Vec::from_raw_parts(ptr, 0, len.max(1)) });
}

#[unsafe(no_mangle)]
pub extern "C" fn orvyn_out_ptr() -> *const u8 {
    OUT.with(|o| o.borrow().as_ptr())
}

#[unsafe(no_mangle)]
pub extern "C" fn orvyn_out_len() -> usize {
    OUT.with(|o| o.borrow().len())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn orvyn_load(ptr: *const u8, len: usize) -> i32 {
    let src = unsafe { read(ptr, len) };
    match Engine::load(&src) {
        Ok(e) => {
            ENGINE.with(|g| *g.borrow_mut() = Some(e));
            set_out(&json!({ "ok": true }));
            0
        }
        Err(problems) => {
            set_out(&json!({ "ok": false, "problems": problems }));
            1
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn orvyn_step(ptr: *const u8, len: usize) -> i32 {
    let raw: Value = serde_json::from_str(&unsafe { read(ptr, len) }).unwrap_or(Value::Null);
    let keys: HashSet<String> = raw["keys"].as_array().map(|a| a.iter().filter_map(|k| k.as_str().map(str::to_lowercase)).collect()).unwrap_or_default();
    let pointer = raw["pointer"].as_array().and_then(|p| Some([p.first()?.as_f64()? as f32, p.get(1)?.as_f64()? as f32]));
    let input = Input { keys, pointer, pointer_down: raw["down"].as_bool().unwrap_or(false) };
    ENGINE.with(|g| match g.borrow_mut().as_mut() {
        Some(e) => {
            let frame = e.step(input);
            let mut v = serde_json::to_value(&frame).unwrap_or(Value::Null);
            if let Some(h) = &e.halted {
                v["halted"] = json!(h);
            }
            set_out(&v);
            0
        }
        None => {
            set_out(&json!({ "error": "no game loaded" }));
            1
        }
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn orvyn_api(ptr: *const u8, len: usize) -> i32 {
    let cmd: Value = serde_json::from_str(&unsafe { read(ptr, len) }).unwrap_or(Value::Null);
    ENGINE.with(|g| match g.borrow_mut().as_mut() {
        Some(e) => {
            set_out(&e.api(&cmd));
            0
        }
        None => {
            set_out(&json!({ "error": "no game loaded" }));
            1
        }
    })
}

/// Play a game headless for `frames` frames with no input: `{ problems, errors, status, frames }`.
/// Used before publishing (with `orvyn_validate`'s checks first).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn orvyn_smoke(ptr: *const u8, len: usize, frames: u32) -> i32 {
    let src = unsafe { read(ptr, len) };
    let mut engine = match Engine::load(&src) {
        Ok(e) => e,
        Err(problems) => {
            set_out(&json!({ "problems": problems, "errors": [], "status": null, "frames": 0 }));
            return 1;
        }
    };
    let mut errors: Vec<String> = Vec::new();
    let mut ran = 0;
    let mut first: Option<Value> = None;
    for _ in 0..frames.min(36_000) {
        let f = engine.step(Input::default());
        ran += 1;
        if first.is_none() {
            first = Some(json!({ "draws": f.draws.len(), "calls": f.calls.iter().map(|c| c.function.clone()).collect::<Vec<_>>() }));
        }
        for e in f.errors {
            if !errors.contains(&e) {
                errors.push(e);
            }
        }
        if engine.halted.is_some() || f.status != crate::world::Status::Running {
            break;
        }
    }
    set_out(&json!({ "problems": [], "errors": errors, "status": engine.status(), "frames": ran, "first": first, "halted": engine.halted }));
    errors.is_empty() as i32 ^ 1
}

/// Problems in a game file, without loading it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn orvyn_validate(ptr: *const u8, len: usize) -> i32 {
    let src = unsafe { read(ptr, len) };
    let problems = match serde_json::from_str::<crate::format::Game>(&src) {
        Ok(g) => crate::validate::problems(&g),
        Err(e) => vec![format!("the game file isn't valid: {e}")],
    };
    set_out(&json!({ "problems": problems }));
    problems.is_empty() as i32 ^ 1
}

fn game_src(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Bots playtest a game: `{ game, options?, sheet?, images? }` → the report (`playtest.rs`), with
/// `sheet` (a base64 PNG contact sheet) when asked. `images` are asset name → base64 PNG.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn orvyn_playtest(ptr: *const u8, len: usize) -> i32 {
    let req: Value = serde_json::from_str(&unsafe { read(ptr, len) }).unwrap_or(Value::Null);
    let src = game_src(&req["game"]);
    let opts: crate::playtest::Options = serde_json::from_value(req["options"].clone()).unwrap_or_default();
    match crate::playtest::playtest(&src, &opts) {
        Ok(report) => {
            let mut v = serde_json::to_value(&report).unwrap_or(Value::Null);
            if req["sheet"].as_bool().unwrap_or(false) {
                let images = images_of(&req["images"]);
                let size = serde_json::from_str::<crate::format::Game>(&src).map(|g| g.size).unwrap_or([480.0, 270.0]);
                if let Some(png) = crate::render::contact_sheet(&report, size, &images) {
                    v["sheet"] = json!(crate::render::base64(&png));
                }
            }
            set_out(&v);
            0
        }
        Err(problems) => {
            set_out(&json!({ "problems": problems }));
            1
        }
    }
}

/// Replay a recorded run: `{ game, log }` → `{ frames, time, status, vars, verifiable, reason }`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn orvyn_replay(ptr: *const u8, len: usize) -> i32 {
    let req: Value = serde_json::from_str(&unsafe { read(ptr, len) }).unwrap_or(Value::Null);
    let log: crate::replay::InputLog = match serde_json::from_value(req["log"].clone()) {
        Ok(l) => l,
        Err(e) => {
            set_out(&json!({ "problems": [format!("the input log isn't valid: {e}")] }));
            return 1;
        }
    };
    match crate::replay::replay(&game_src(&req["game"]), &log) {
        Ok(r) => {
            set_out(&serde_json::to_value(&r).unwrap_or(Value::Null));
            0
        }
        Err(problems) => {
            set_out(&json!({ "problems": problems }));
            1
        }
    }
}

fn images_of(v: &Value) -> crate::render::Images {
    v.as_object()
        .map(|m| m.iter().filter_map(|(k, b)| Some((k.clone(), crate::render::unbase64(b.as_str()?)?))).collect())
        .unwrap_or_default()
}
