//! Problems in a game file, found before it runs or is published: everything a rule, tilemap or
//! entity refers to must exist, sizes must be sane, and scripts must compile. Messages are written
//! for whoever is making the game (a person in the editor, or an agent).

use std::collections::BTreeSet;

use crate::format::*;

const MAX_ENTITIES: usize = 5_000;
const KEYS: &[&str] = &["left", "right", "up", "down", "space", "enter", "shift", "escape", "tab", "backspace"];

fn key_ok(k: &str) -> bool {
    KEYS.contains(&k) || (k.len() == 1 && k.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()))
}

fn color_ok(c: &str) -> bool {
    let hex = c.strip_prefix('#').is_some_and(|h| [3, 4, 6, 8].contains(&h.len()) && h.chars().all(|x| x.is_ascii_hexdigit()));
    hex || c.starts_with("rgb(") || c.starts_with("rgba(") || c.starts_with("hsl(") || (c.len() <= 20 && c.chars().all(|x| x.is_ascii_lowercase()))
}

pub fn problems(g: &Game) -> Vec<String> {
    let mut p = Vec::new();
    if g.format != FORMAT {
        p.push(format!("format must be \"{FORMAT}\""));
    }
    if g.title.trim().is_empty() || g.title.len() > 80 {
        p.push("give the game a title (up to 80 characters)".into());
    }
    if !(64.0..=1920.0).contains(&g.size[0]) || !(64.0..=1920.0).contains(&g.size[1]) {
        p.push("size must be between 64 and 1920 pixels each way".into());
    }
    if !g.scenes.contains_key(&g.start) {
        p.push(format!("start scene \"{}\" doesn't exist", g.start));
    }
    if !color_ok(&g.background) {
        p.push(format!("background \"{}\" isn't a colour", g.background));
    }
    if let Some(lb) = &g.leaderboard {
        if lb.var != "$time" && !g.vars.contains_key(&lb.var) {
            p.push(format!("the leaderboard uses \"{}\", which isn't one of the game's vars (or use \"$time\")", lb.var));
        }
        if lb.order != "high" && lb.order != "low" {
            p.push("leaderboard order must be \"high\" or \"low\"".into());
        }
        if g.save.contains(&lb.var) {
            p.push(format!("\"{}\" can't be both saved and on the leaderboard: a saved score would carry into the next run", lb.var));
        }
        if g.scripts.keys().any(|k| k.ends_with(".ts") || k.ends_with(".js")) {
            p.push("a game with a leaderboard can't use TypeScript/JavaScript scripts (Orvyn checks scores by replaying the game in the engine; use rules or Rhai)".into());
        }
    }
    for v in &g.save {
        if !g.vars.contains_key(v) {
            p.push(format!("save lists \"{v}\", which isn't one of the game's vars"));
        }
    }
    if g.save.len() > 32 {
        p.push("save can list at most 32 variables".into());
    }
    let images: BTreeSet<&String> = g.assets.images.keys().collect();
    let sounds: BTreeSet<&String> = g.assets.sounds.keys().collect();
    for (name, img) in &g.assets.images {
        for (anim, a) in &img.anims {
            if a.frames.is_empty() || a.fps <= 0.0 {
                p.push(format!("image {name}: animation {anim} needs frames and a positive fps"));
            }
            if img.frame.is_none() {
                p.push(format!("image {name}: animation {anim} needs the sheet's frame size (\"frame\": [w, h])"));
            }
        }
        if img.src.trim().is_empty() {
            p.push(format!("image {name} has no src"));
        }
    }

    let check_entity = |where_: &str, e: &EntityDef, p: &mut Vec<String>| {
        if let Some(pf) = &e.prefab {
            if !g.prefabs.contains_key(pf) {
                p.push(format!("{where_}: prefab \"{pf}\" doesn't exist"));
            }
        }
        if let Some(s) = &e.sprite {
            if !images.contains(&s.image) {
                p.push(format!("{where_}: image \"{}\" isn't in assets.images", s.image));
            } else if let Some(a) = &s.anim {
                if !g.assets.images[&s.image].anims.contains_key(a) {
                    p.push(format!("{where_}: image {} has no animation \"{a}\"", s.image));
                }
            }
        }
        if let Some(c) = &e.color {
            if !color_ok(c) {
                p.push(format!("{where_}: \"{c}\" isn't a colour"));
            }
        }
        if let Some(b) = &e.body {
            if !["none", "static", "dynamic", "kinematic"].contains(&b.as_str()) {
                p.push(format!("{where_}: body is none, static, dynamic or kinematic"));
            }
        }
        if let Some(s) = e.size {
            if s[0] <= 0.0 || s[1] <= 0.0 || s[0] > 10_000.0 || s[1] > 10_000.0 {
                p.push(format!("{where_}: size must be positive (and under 10000)"));
            }
        }
        for b in &e.behaviours {
            if let Behaviour::Follow { target, .. } = b {
                if target.is_empty() {
                    p.push(format!("{where_}: follow needs a target tag"));
                }
            }
        }
    };
    for (name, pf) in &g.prefabs {
        if pf.prefab.is_some() {
            p.push(format!("prefab {name}: prefabs can't be based on other prefabs"));
        }
        check_entity(&format!("prefab {name}"), pf, &mut p);
    }

    for (sname, scene) in &g.scenes {
        let at = |what: String| format!("scene {sname}: {what}");
        let mut count = scene.entities.len();
        for (i, e) in scene.entities.iter().enumerate() {
            check_entity(&at(format!("entity {}", e.id.clone().unwrap_or((i + 1).to_string()))), e, &mut p);
        }
        if let Some(map) = &scene.tilemap {
            if map.tile[0] <= 0.0 || map.tile[1] <= 0.0 {
                p.push(at("tilemap tile size must be positive".into()));
            }
            for (k, t) in &map.legend {
                if k.chars().count() != 1 {
                    p.push(at(format!("tilemap legend keys are single characters (\"{k}\")")));
                }
                match t {
                    TileDef::Prefab(pf) if !g.prefabs.contains_key(pf) => p.push(at(format!("tilemap \"{k}\": prefab \"{pf}\" doesn't exist"))),
                    TileDef::Tile { image: Some(i), .. } if !images.contains(i) => p.push(at(format!("tilemap \"{k}\": image \"{i}\" isn't in assets.images"))),
                    TileDef::Tile { color: Some(c), .. } if !color_ok(c) => p.push(at(format!("tilemap \"{k}\": \"{c}\" isn't a colour"))),
                    _ => {}
                }
            }
            count += map.rows.iter().map(|r| r.chars().count()).sum::<usize>();
        }
        if count > MAX_ENTITIES {
            p.push(at(format!("{count} entities; a scene can have at most {MAX_ENTITIES}")));
        }
        for (i, rule) in scene.rules.iter().enumerate() {
            let r = |what: String| at(format!("rule {}: {what}", i + 1));
            match &rule.when {
                Event::Every { seconds } if *seconds <= 0.0 => p.push(r("every needs positive seconds".into())),
                Event::Key { key, state } => {
                    if !key_ok(key) {
                        p.push(r(format!("unknown key \"{key}\" (use left, right, up, down, space, enter, a–z, 0–9…)")));
                    }
                    if !["pressed", "down", "released"].contains(&state.as_str()) {
                        p.push(r("key state is pressed, down or released".into()));
                    }
                }
                Event::Var { op, .. } if !["==", "=", "!=", ">", ">=", "<", "<="].contains(&op.as_str()) => p.push(r(format!("unknown comparison \"{op}\""))),
                _ => {}
            }
            if rule.actions.is_empty() {
                p.push(r("does nothing (no actions)".into()));
            }
            for a in &rule.actions {
                match a {
                    Action::Spawn { prefab, .. } if !g.prefabs.contains_key(prefab) => p.push(r(format!("spawn: prefab \"{prefab}\" doesn't exist"))),
                    Action::Sound { sound } if !sounds.contains(sound) => p.push(r(format!("sound \"{sound}\" isn't in assets.sounds"))),
                    Action::Scene { scene } if !g.scenes.contains_key(scene) => p.push(r(format!("scene \"{scene}\" doesn't exist"))),
                    Action::Call { function } if g.scripts.is_empty() => p.push(r(format!("calls {function}, but the game has no scripts"))),
                    _ => {}
                }
            }
        }
    }
    for (name, src) in &g.scripts {
        let ok_ext = [".rhai", ".ts", ".js"].iter().any(|e| name.ends_with(e));
        if !ok_ext {
            p.push(format!("script {name}: scripts are .rhai (run everywhere) or .ts/.js (run in the browser player)"));
        }
        if src.len() > 200_000 {
            p.push(format!("script {name} is over 200 KB"));
        }
        if name.ends_with(".rhai") {
            if let Err(e) = rhai::Engine::new().compile(src) {
                p.push(format!("script {name}: {e}"));
            }
        }
    }
    p
}
