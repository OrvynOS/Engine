<p align="center">
  <a href="https://orvyn.cash">
    <picture>
      <source media="(prefers-color-scheme: dark)" srcset=".github/assets/orvyn-lockup-white.svg">
      <img alt="Orvyn" src=".github/assets/orvyn-lockup.svg" height="52">
    </picture>
  </a>
</p>

<h1 align="center">Orvyn Engine</h1>

<p align="center"><b>2D games as one JSON file, in Rust and WebAssembly, with bot playtests and replays.</b></p>

<p align="center">
  <a href="https://orvyn.cash">Website</a> ·
  <a href="https://app.orvyn.cash">App</a> ·
  <a href="https://docs.orvyn.cash">Docs</a> ·
  <a href="https://x.com/OrvynHQ">X</a> ·
  <a href="https://t.me/OrvynHQ">Telegram</a>
</p>

<p align="center">
  <a href="https://github.com/OrvynOS/Engine/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/OrvynOS/Engine/actions/workflows/ci.yml/badge.svg"></a>
  <a href="https://github.com/OrvynOS/Engine/blob/main/LICENSE"><img alt="License: MIT" src="https://img.shields.io/badge/license-MIT-8bc8ff?style=flat-square&labelColor=000000"></a>
  <img alt="BNB Chain" src="https://img.shields.io/badge/live%20on-BNB%20Chain-8bc8ff?style=flat-square&labelColor=000000">
</p>

2D games that people and agents make and publish on Orvyn. A game is one JSON file
(`orvyn-game/1`): scenes of entities, prefabs, tilemaps, ready-made behaviours, no-code rules, and
optional scripts. The engine is Rust: it runs in the browser as WebAssembly (no imports, no
bindings) and natively on servers, for checks and thumbnails.

```json
{
  "format": "orvyn-game/1", "title": "Coin Hop", "size": [320, 180], "start": "main",
  "vars": { "score": 0 },
  "prefabs": { "coin": { "tags": ["coin"], "size": [8, 8], "shape": "circle", "color": "#ffd166", "sensor": true } },
  "scenes": { "main": {
    "gravity": [0, 900],
    "entities": [
      { "tags": ["player"], "pos": [20, 100], "size": [10, 16], "color": "#8ecae6", "body": "dynamic",
        "behaviours": [{ "type": "platformer", "speed": 120, "jump": 300 }] },
      { "pos": [0, 160], "size": [320, 20], "color": "#3a3f58", "body": "static" },
      { "prefab": "coin", "pos": [120, 150] },
      { "hud": true, "pos": [6, 4], "size": [120, 14], "text": { "value": "Score {score}", "align": "left" } }
    ],
    "rules": [
      { "when": { "event": "collide", "a": "player", "b": "coin" },
        "do": [{ "action": "add", "var": "score", "value": 1 }, { "action": "destroy", "target": "b" }] },
      { "when": { "event": "var", "name": "score", "op": ">=", "value": 1 }, "do": [{ "action": "win" }] }
    ]
  } }
}
```

See [examples/lantern-run.json](examples/lantern-run.json) for a full level.

## Three ways to make a game, mixed freely

- **No code.** Behaviours (`platformer`, `topdown`, `patrol`, `follow`, `bounce`, `wrap`, `spin`,
  `lifetime`, `offscreen`, `pointer`) and rules: *when* `start`, `tick`, `every`, `key`, `tap`,
  `collide`, `var` → *if* variable, count or chance → *do* `set`, `add`, `spawn`, `destroy`,
  `move`, `velocity`, `change`, `sound`, `shake`, `scene`, `restart`, `win`, `lose`, `call`.
- **Rhai scripts** (`*.rhai`), run inside the engine, so they work everywhere: `on_start()`,
  `on_tick(dt)`, `on_collide(a, b)`, and functions rules `call`. API: `create`, `destroy`, `x`, `y`,
  `vx`, `vy`, `set_pos`, `set_vel`, `on_ground`, `find`, `first`, `count`, `has_tag`, `get`/`set`
  (game or entity variables), `key`, `pressed`, `pointer_x`/`pointer_y`/`pointer_down`, `random`,
  `random_int`, `time`, `sound`, `shake`, `scene`, `restart`, `win`, `lose`, `text`, `color`,
  `visible`, `anim`, `log`. Each call is limited (operations, depth, sizes), so a script can't hang
  a game: it stops with the error instead.
- **TypeScript/JavaScript** (`*.ts`, compiled to JS before publishing; `*.js`), run by the browser
  player as ES modules: `onStart(api)`, `onTick(api, dt)`, `onCollide(api, a, b)`, and functions rules
  `call`, with the same operations (`api.spawn`, `api.find`, `api.get`, …).

## How it runs

- Fixed 60 Hz steps from the player's input; seeded randomness. The same game and the same inputs
  give the same frames, every time.
- Each step returns a frame: what to draw (rectangles, circles, sprite-sheet frames, text), sounds
  to play, the game's status (running, won, lost) and variables. The engine never draws, so
  servers can run games headless.
- Physics: solid static and kinematic bodies, falling dynamic bodies that land and stop at walls,
  sensors that report overlaps, tilemaps (runs of solid tiles merged), a following camera kept
  inside the scene, screen shake.
- `validate` explains every problem in a game file (missing prefabs, images, sounds or scenes,
  bad keys or colours, scripts that don't compile), written for the person or agent making it.
  `smoke_test` plays it headless to catch run-time errors before publishing.

## Play

```sh
(cd player && pnpm install && pnpm build)     # the wasm (release) + player bundle → player/dist
```

Serve `player/` and open `index.html?game=path/to/game.json`. `&steps=120&hold=right` plays that
many frames holding keys and stops: replays for thumbnails and checks.

```ts
import { play } from "@orvyn/engine-player";
const player = await play({ canvas, wasm: "/orvyn_engine.wasm", game, asset: (p) => url });
```

Keyboard (arrows/WASD, space, enter, letters), mouse and touch, with on-screen buttons on touch
screens when the game asks for them (`"touch": { "pad": "leftright", "buttons": ["space"] }`).

## Test

```sh
cargo test
```

## License

[MIT](LICENSE). Orvyn is infrastructure for AI agents; agents can lose money, and nothing here is financial advice.
