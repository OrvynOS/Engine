//! Orvyn Engine, checked headless: physics and behaviours, no-code rules, Rhai scripts (and their
//! limits), determinism, validation, and the example game.

use std::collections::HashSet;

use orvyn_engine::world::{Input, Status};
use orvyn_engine::{smoke_test, Engine};
use serde_json::{json, Value};

fn game(scene: Value, extra: Value) -> String {
    let mut g = json!({
        "format": "orvyn-game/1", "title": "Test", "size": [320, 180], "start": "main",
        "vars": { "score": 0 },
        "prefabs": {
            "coin": { "tags": ["coin"], "size": [8, 8], "shape": "circle", "color": "#ff0", "sensor": true },
            "bullet": { "tags": ["bullet"], "size": [4, 4], "color": "#fff", "vel": [200, 0], "behaviours": [{ "type": "offscreen" }] }
        },
        "scenes": { "main": scene },
        "assets": { "sounds": { "ding": "ding.wav" } }
    });
    if let Value::Object(m) = extra {
        for (k, v) in m {
            g[k] = v;
        }
    }
    g.to_string()
}

fn keys(k: &[&str]) -> Input {
    Input { keys: k.iter().map(|s| s.to_string()).collect::<HashSet<_>>(), pointer: None, pointer_down: false }
}

fn run(e: &mut Engine, frames: u32, input: &[&str]) {
    for _ in 0..frames {
        e.step(keys(input));
    }
}

fn entity_x(e: &Engine, id: &str) -> f32 {
    e.world().entities.iter().find(|x| x.id.as_deref() == Some(id)).map(|x| x.pos[0]).unwrap()
}
fn entity_y(e: &Engine, id: &str) -> f32 {
    e.world().entities.iter().find(|x| x.id.as_deref() == Some(id)).map(|x| x.pos[1]).unwrap()
}

fn platform_scene(rules: Value, more: Value) -> Value {
    let mut entities = vec![
        json!({ "id": "hero", "tags": ["player"], "pos": [20, 100], "size": [10, 16], "color": "#0af", "body": "dynamic", "behaviours": [{ "type": "platformer", "speed": 120, "jump": 300 }] }),
        json!({ "id": "floor", "pos": [0, 160], "size": [320, 20], "color": "#444", "body": "static" }),
    ];
    if let Value::Array(m) = more {
        entities.extend(m);
    }
    json!({ "gravity": [0, 900], "entities": entities, "rules": rules })
}

#[test]
fn falls_lands_runs_and_jumps() {
    let mut e = Engine::load(&game(platform_scene(json!([]), json!([])), json!({}))).unwrap();
    run(&mut e, 60, &[]);
    assert!((entity_y(&e, "hero") - 144.0).abs() < 0.01, "lands on the floor: y={}", entity_y(&e, "hero"));
    assert!(e.world().entities.iter().find(|x| x.id.as_deref() == Some("hero")).unwrap().on_ground);
    let x0 = entity_x(&e, "hero");
    run(&mut e, 30, &["right"]);
    assert!((entity_x(&e, "hero") - x0 - 60.0).abs() < 0.5, "runs at 120 px/s: moved {}", entity_x(&e, "hero") - x0);
    e.step(keys(&["up"]));
    run(&mut e, 10, &["up"]);
    assert!(entity_y(&e, "hero") < 144.0 - 20.0, "jumps: y={}", entity_y(&e, "hero"));
    run(&mut e, 60, &[]);
    assert!((entity_y(&e, "hero") - 144.0).abs() < 0.01, "lands again");
    // Holding jump doesn't bounce: only a fresh press jumps.
    run(&mut e, 40, &["up"]);
    assert!((entity_y(&e, "hero") - 144.0).abs() < 0.01);
}

#[test]
fn walls_stop_and_patrols_turn() {
    let more = json!([
        { "id": "wall", "pos": [100, 100], "size": [10, 60], "color": "#888", "body": "static" },
        { "id": "guard", "pos": [200, 140], "size": [10, 20], "color": "#f00", "body": "dynamic", "behaviours": [{ "type": "patrol", "speed": 60, "range": 30 }] }
    ]);
    let mut e = Engine::load(&game(platform_scene(json!([]), more), json!({}))).unwrap();
    run(&mut e, 120, &["right"]);
    assert!((entity_x(&e, "hero") - 90.0).abs() < 0.01, "stopped by the wall at x=90: {}", entity_x(&e, "hero"));
    let xs: Vec<f32> = (0..240).map(|_| {
        e.step(keys(&[]));
        entity_x(&e, "guard")
    }).collect();
    let (min, max) = (xs.iter().cloned().fold(f32::MAX, f32::min), xs.iter().cloned().fold(f32::MIN, f32::max));
    assert!(min >= 169.0 && max <= 231.0 && max - min > 50.0, "patrols within its range: {min}..{max}");
}

#[test]
fn collide_rules_score_destroy_and_play_sounds() {
    let rules = json!([
        { "when": { "event": "collide", "a": "player", "b": "coin" }, "do": [
            { "action": "add", "var": "score", "value": 1 }, { "action": "destroy", "target": "b" }, { "action": "sound", "sound": "ding" } ] },
        { "when": { "event": "var", "name": "score", "op": ">=", "value": 2 }, "do": [{ "action": "win", "message": "Got {score}" }] }
    ]);
    let coins = json!([{ "prefab": "coin", "pos": [60, 150] }, { "prefab": "coin", "pos": [100, 150] }]);
    let mut e = Engine::load(&game(platform_scene(rules, coins), json!({}))).unwrap();
    let mut sounds = 0;
    for _ in 0..200 {
        let f = e.step(keys(&["right"]));
        sounds += f.sounds.iter().filter(|s| *s == "ding").count();
        if f.status != Status::Running {
            break;
        }
    }
    assert_eq!(e.vars()["score"], json!(2));
    assert_eq!(sounds, 2);
    assert_eq!(e.world().with_tag("coin").len(), 0);
    assert_eq!(e.status(), Status::Won);
    assert_eq!(e.world().message.as_deref(), Some("Got 2"));
}

#[test]
fn timers_keys_taps_and_spawns() {
    let scene = json!({
        "entities": [
            { "id": "gun", "pos": [10, 80], "size": [10, 10], "color": "#fff" },
            { "id": "button", "tags": ["button"], "hud": true, "pos": [280, 10], "size": [30, 20], "color": "#0f0" }
        ],
        "rules": [
            { "when": { "event": "every", "seconds": 0.5 }, "do": [{ "action": "add", "var": "ticks", "value": 1 }] },
            { "when": { "event": "key", "key": "space" }, "do": [{ "action": "spawn", "prefab": "bullet", "at": "gun" }] },
            { "when": { "event": "tap", "tag": "button" }, "do": [{ "action": "add", "var": "taps", "value": 1 }] }
        ]
    });
    let mut e = Engine::load(&game(scene, json!({ "vars": { "ticks": 0, "taps": 0 } }))).unwrap();
    run(&mut e, 120, &[]);
    assert_eq!(e.vars()["ticks"], json!(4), "every 0.5s over 2s");
    e.step(keys(&["space"]));
    run(&mut e, 5, &["space"]);
    assert_eq!(e.world().with_tag("bullet").len(), 1, "one press, one bullet");
    run(&mut e, 120, &[]);
    assert_eq!(e.world().with_tag("bullet").len(), 0, "bullets leave the screen and are removed");
    let tap = |x: f32, y: f32, down: bool| Input { keys: HashSet::new(), pointer: Some([x, y]), pointer_down: down };
    e.step(tap(290.0, 15.0, true));
    e.step(tap(290.0, 15.0, false));
    e.step(tap(10.0, 150.0, true));
    assert_eq!(e.vars()["taps"], json!(1), "only taps on the button count");
}

#[test]
fn rhai_scripts_and_their_limits() {
    let scripts = json!({ "main.rhai": r#"
        fn on_start() { set("ready", true); create("coin", 50.0, 50.0); }
        fn on_tick(dt) { if time() > 0.5 && get("half") == () { set("half", count("coin")); } }
        fn bonus(a, b) { set("score", get("score") + 10); destroy(b); }
    "# });
    let scene = json!({ "entities": [{ "id": "hero", "tags": ["player"], "pos": [0, 50], "size": [10, 10], "color": "#fff", "vel": [60, 0] }],
        "rules": [{ "when": { "event": "collide", "a": "player", "b": "coin" }, "do": [{ "action": "call", "function": "bonus" }] }] });
    let mut e = Engine::load(&game(scene, json!({ "scripts": scripts }))).unwrap();
    run(&mut e, 60, &[]);
    assert_eq!(e.vars()["ready"], json!(true));
    assert_eq!(e.vars()["half"], json!(1), "on_tick saw one coin at 0.5s");
    assert_eq!(e.vars()["score"], json!(10), "the rule called the script, which scored and destroyed the coin");
    assert_eq!(e.world().with_tag("coin").len(), 0);

    // A runaway script stops the game with an error instead of hanging it.
    let spin = json!({ "loop.rhai": "fn on_tick(dt) { loop { } }" });
    let mut e = Engine::load(&game(json!({ "entities": [] }), json!({ "scripts": spin }))).unwrap();
    let f = e.step(Input::default());
    assert!(e.halted.is_some() && !f.errors.is_empty(), "halted: {:?}", e.halted);

    // TS/JS calls go to the player.
    let scene = json!({ "entities": [], "rules": [{ "when": { "event": "start" }, "do": [{ "action": "call", "function": "intro" }] }] });
    let mut e = Engine::load(&game(scene, json!({ "scripts": { "main.ts": "export function intro() {}" } }))).unwrap();
    let f = e.step(Input::default());
    assert_eq!(f.calls.first().map(|c| c.function.as_str()), Some("intro"));
}

#[test]
fn same_inputs_same_game() {
    let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/examples/lantern-run.json")).unwrap();
    let play = || {
        let mut e = Engine::load(&src).unwrap();
        let mut out = Vec::new();
        for i in 0..600u32 {
            let k: &[&str] = match i % 120 {
                0..=60 => &["right"],
                61 => &["up"],
                62..=90 => &["right", "up"],
                _ => &[],
            };
            out.push(serde_json::to_string(&e.step(keys(k))).unwrap());
        }
        out
    };
    assert!(play() == play(), "two runs with the same inputs differ");
}

#[test]
fn the_example_game_plays() {
    let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/examples/lantern-run.json")).unwrap();
    assert_eq!(smoke_test(&src, 600).unwrap(), Vec::<String>::new());
    let mut e = Engine::load(&src).unwrap();
    let f = e.step(Input::default());
    assert!(f.draws.len() > 10, "draws the level");
    assert!(serde_json::to_string(&f).unwrap().contains("Sparks 0   Lives 3"), "HUD text filled in");
    // The camera follows the player (and stays inside the scene's bounds).
    let hero = e.api(&json!({ "op": "find", "tag": "player" }))[0].as_u64().unwrap();
    e.api(&json!({ "op": "set_pos", "id": hero, "x": 600, "y": 60 }));
    e.step(Input::default());
    let cam = e.world().camera;
    assert!((cam[0] - (608.0 - 240.0)).abs() < 1.0 && cam[1] == 0.0, "camera at {cam:?}");
    e.api(&json!({ "op": "set_pos", "id": hero, "x": 950, "y": 60 }));
    e.step(Input::default());
    assert_eq!(e.world().camera[0], 960.0 - 480.0, "clamped to the right edge");
}

#[test]
fn validation_explains_problems() {
    let bad = json!({
        "format": "orvyn-game/1", "title": "", "start": "nope", "size": [10, 10],
        "prefabs": { "a": { "sprite": { "image": "missing" } } },
        "scenes": { "main": {
            "entities": [{ "prefab": "ghost" }],
            "tilemap": { "tile": [16, 16], "rows": ["xy"], "legend": { "x": "nothing", "yy": { "color": "#fff" } } },
            "rules": [
                { "when": { "event": "key", "key": "jump" }, "do": [{ "action": "sound", "sound": "boom" }] },
                { "when": { "event": "start" }, "do": [{ "action": "scene", "scene": "nowhere" }, { "action": "spawn", "prefab": "ufo" }] }
            ]
        } },
        "scripts": { "bad.rhai": "fn x( {", "notes.txt": "" }
    });
    let problems = Engine::load(&bad.to_string()).err().unwrap();
    let all = problems.join("\n");
    for expect in ["title", "start scene \"nope\"", "size must be", "image \"missing\"", "prefab \"ghost\"", "prefab \"nothing\"", "single characters", "unknown key \"jump\"", "sound \"boom\"", "scene \"nowhere\"", "prefab \"ufo\"", "bad.rhai", "notes.txt"] {
        assert!(all.contains(expect), "missing problem {expect:?} in:\n{all}");
    }
}
