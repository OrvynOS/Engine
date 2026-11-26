//! Playtesting, replays and the snapshot renderer.

use orvyn_engine::playtest::{playtest, Options};
use orvyn_engine::render::{contact_sheet, frame_png, Images};
use orvyn_engine::replay::{replay, InputLog};
use orvyn_engine::world::{Input, Status};
use orvyn_engine::Engine;

const LANTERN: &str = include_str!("../examples/lantern-run.json");

fn opts(seconds: f32) -> Options {
    Options { seconds, snapshot_every: 4.0, bots: vec![] }
}

#[test]
fn bots_play_the_example_and_agree_with_themselves() {
    let a = playtest(LANTERN, &opts(20.0)).expect("playtest");
    assert_eq!(a.runs.len(), 3);
    assert!(a.keys.contains(&"right".to_string()), "keys: {:?}", a.keys);
    assert!(a.runs.iter().all(|r| r.errors.is_empty()), "errors: {:?}", a.runs.iter().map(|r| &r.errors).collect::<Vec<_>>());
    let explorer = a.runs.iter().find(|r| r.bot == "explorer").unwrap();
    let t = explorer.player.as_ref().expect("the explorer moves a player");
    assert!(t.farthest > 40.0, "the explorer went somewhere ({} px)", t.farthest);
    assert!(!a.findings.is_empty());
    // Deterministic: the same game gives the same report.
    let b = playtest(LANTERN, &opts(20.0)).unwrap();
    assert_eq!(serde_json::to_string(&a.runs.iter().map(|r| (&r.bot, &r.vars, r.seconds)).collect::<Vec<_>>()).unwrap(), serde_json::to_string(&b.runs.iter().map(|r| (&r.bot, &r.vars, r.seconds)).collect::<Vec<_>>()).unwrap());
    println!("findings: {:#?}", a.findings);
}

#[test]
fn bots_find_a_bottomless_level_and_a_game_that_never_ends() {
    let game = r##"{
      "format": "orvyn-game/1", "title": "Void", "size": [320, 180], "start": "a", "vars": { "score": 0 },
      "scenes": { "a": { "gravity": [0, 900], "entities": [
        { "id": "hero", "tags": ["player"], "pos": [40, 40], "size": [12, 16], "color": "#fff", "body": "dynamic", "behaviours": [{ "type": "platformer" }] }
      ], "rules": [] } } }"##;
    let r = playtest(game, &opts(10.0)).expect("playtest");
    let all = r.findings.join("\n");
    assert!(all.contains("fell out of the level"), "{all}");
    assert!(all.contains("never ends"), "{all}");
    assert!(all.contains("“score” never changed"), "{all}");
}

#[test]
fn a_replay_of_recorded_input_gives_the_same_result() {
    // Play directly: hold right for two seconds, then jump.
    let mut e = Engine::load(LANTERN).unwrap();
    let mut log = InputLog { v: 1, frames: 0, ..Default::default() };
    for f in 0..240u64 {
        let mut input = Input::default();
        if f < 120 {
            input.keys.insert("right".into());
        }
        if f == 120 {
            input.keys.insert("up".into());
        }
        if f == 0 {
            log.keys.push((0, "+right".into()));
        }
        if f == 120 {
            log.keys.push((120, "-right".into()));
            log.keys.push((120, "+up".into()));
        }
        if f == 121 {
            log.keys.push((121, "-up".into()));
        }
        let fr = e.step(input);
        log.frames = f + 1;
        if fr.status != Status::Running {
            break;
        }
    }
    let direct = (e.world().vars.clone(), e.world().entities.iter().map(|x| x.pos).collect::<Vec<_>>(), e.world().frame);
    let r = replay(LANTERN, &log).expect("replay");
    assert!(r.verifiable);
    assert_eq!(r.vars, direct.0);
    assert_eq!(r.frames, direct.2);
}

#[test]
fn renders_frames_and_a_contact_sheet() {
    let mut e = Engine::load(LANTERN).unwrap();
    let f = e.step(Input::default());
    let size = e.world().game.size;
    let png = frame_png(&f, size, 1.0, &Images::new()).expect("png");
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    let report = playtest(LANTERN, &opts(12.0)).unwrap();
    let sheet = contact_sheet(&report, size, &Images::new()).expect("sheet");
    assert_eq!(&sheet[..4], b"\x89PNG");
    let out = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("lantern-sheet.png");
    std::fs::write(&out, &sheet).unwrap();
    println!("contact sheet: {}", out.display());
}

#[test]
fn base64_round_trips() {
    use orvyn_engine::render::{base64, unbase64};
    for s in [&b""[..], b"a", b"ab", b"abc", b"\x00\xff\x10orvyn"] {
        assert_eq!(unbase64(&base64(s)).unwrap(), s);
    }
}
