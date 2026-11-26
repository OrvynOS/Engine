/**
 * The Orvyn Engine player: runs a game (orvyn-game/1) in a canvas.
 *
 * The engine (Rust, WebAssembly) does the game; this draws its frames, reads keys, mouse and touch,
 * plays sounds, and runs the game's TypeScript/JavaScript scripts (compiled to JS before publishing)
 * through the same API Rhai scripts use.
 *
 *   const player = await play({ canvas, wasm: "/orvyn_engine.wasm", game, asset: (path) => url });
 */

export interface Game {
  format: string;
  title: string;
  size?: [number, number];
  background?: string;
  assets?: { images?: Record<string, { src: string; frame?: [number, number] }>; sounds?: Record<string, string> };
  scripts?: Record<string, string>;
  touch?: { pad?: "dpad" | "leftright" | null; buttons?: string[] };
  leaderboard?: { var: string; order?: "high" | "low"; label?: string };
  save?: string[];
  [k: string]: unknown;
}

type Draw =
  | { k: "r"; x: number; y: number; w: number; h: number; c: string; a: number; rot: number }
  | { k: "o"; x: number; y: number; w: number; h: number; c: string; a: number }
  | { k: "i"; img: string; f: number; fw: number; fh: number; x: number; y: number; w: number; h: number; a: number; rot: number; flip: boolean }
  | { k: "t"; s: string; x: number; y: number; w: number; size: number; c: string; align: string; a: number };

export interface Frame {
  frame: number;
  time: number;
  background: string;
  draws: Draw[];
  sounds: string[];
  status: "running" | "won" | "lost";
  message: string | null;
  vars: Record<string, unknown>;
  calls: { function: string; a: number | null; b: number | null }[];
  contacts: [number, number][];
  logs: string[];
  errors: string[];
  halted?: string;
}

/** What scripts get: the same operations as Rhai scripts. */
export interface Api {
  spawn(prefab: string, x: number, y: number): number | null;
  destroy(id: number): void;
  entity(id: number): { id: number; name: string | null; tags: string[]; x: number; y: number; w: number; h: number; vx: number; vy: number; onGround: boolean; vars: Record<string, unknown> } | null;
  setPos(id: number, x: number, y: number): void;
  setVel(id: number, x: number, y: number): void;
  find(tag: string): number[];
  get(name: string, id?: number): unknown;
  set(name: string, value: unknown, id?: number): void;
  random(): number;
  sound(name: string): void;
  scene(name: string): void;
  win(message?: string): void;
  lose(message?: string): void;
  text(id: number, text: string): void;
  key(key: string): boolean;
  pressed(key: string): boolean;
}

/** A game script module: any of these, plus functions rules call by name (`fn(api, a, b)`). */
export interface ScriptModule {
  onStart?(api: Api): void;
  onTick?(api: Api, dt: number): void;
  onCollide?(api: Api, a: number, b: number): void;
  [name: string]: unknown;
}

export interface PlayOptions {
  canvas: HTMLCanvasElement;
  /** URL of orvyn_engine.wasm, or its bytes. */
  wasm: string | ArrayBuffer;
  game: Game | string;
  /** Where a game file (image, sound) is: default, relative to the page. */
  asset?: (path: string) => string;
  /** Show touch controls (default: on touch screens, if the game asks for them). */
  touchControls?: boolean;
  onFrame?: (f: Frame) => void;
  onEnd?: (status: "won" | "lost", message: string | null) => void;
  onError?: (message: string) => void;
  /**
   * The player's saved variables (only those in the game's `save` list) to start each run with,
   * and where changes go. Call `confirmSave` once a change is stored, so the next run starts from
   * what's stored (and its replay can be checked).
   */
  save?: { data: Record<string, unknown>; onChange?: (data: Record<string, unknown>) => void };
  /** A run ended: its result and the exact input that produced it (leaderboards replay it). */
  onRun?: (run: Run) => void;
}

/** What the player did, as changes per engine step (crates/engine/src/replay.rs). */
export interface InputLog {
  v: 1;
  frames: number;
  keys: [number, string][];
  pointer: [number, number | null, number | null, boolean][];
  start: Record<string, unknown>;
}

export interface Run {
  status: "won" | "lost";
  message: string | null;
  time: number;
  vars: Record<string, unknown>;
  log: InputLog;
}

const KEY_NAMES: Record<string, string> = {
  ArrowLeft: "left",
  ArrowRight: "right",
  ArrowUp: "up",
  ArrowDown: "down",
  " ": "space",
  Spacebar: "space",
  Enter: "enter",
  Shift: "shift",
  Escape: "escape",
  Tab: "tab",
  Backspace: "backspace",
};
const keyName = (e: KeyboardEvent) => KEY_NAMES[e.key] ?? (e.key.length === 1 ? e.key.toLowerCase() : null);

const enc = new TextEncoder();
const dec = new TextDecoder();

interface Exports {
  memory: WebAssembly.Memory;
  orvyn_alloc(len: number): number;
  orvyn_free(ptr: number, len: number): void;
  orvyn_out_ptr(): number;
  orvyn_out_len(): number;
  orvyn_load(ptr: number, len: number): number;
  orvyn_step(ptr: number, len: number): number;
  orvyn_api(ptr: number, len: number): number;
  orvyn_validate(ptr: number, len: number): number;
}
type Fn = "orvyn_load" | "orvyn_step" | "orvyn_api" | "orvyn_validate";

class Wasm {
  constructor(private x: Exports) {}

  call(fn: Fn, input: string): [number, unknown] {
    const bytes = enc.encode(input);
    const ptr = this.x.orvyn_alloc(bytes.length);
    new Uint8Array(this.x.memory.buffer, ptr, bytes.length).set(bytes);
    const code = this.x[fn](ptr, bytes.length);
    this.x.orvyn_free(ptr, bytes.length);
    const out = new Uint8Array(this.x.memory.buffer, this.x.orvyn_out_ptr(), this.x.orvyn_out_len());
    return [code, JSON.parse(dec.decode(out))];
  }
}

async function loadWasm(source: string | ArrayBuffer) {
  const bytes = typeof source === "string" ? await (await fetch(source)).arrayBuffer() : source;
  const { instance } = await WebAssembly.instantiate(bytes, {});
  return new Wasm(instance.exports as unknown as Exports);
}

/** Problems in a game file (the same checks publishing runs). */
export async function validate(wasm: string | ArrayBuffer, game: Game | string): Promise<string[]> {
  const w = await loadWasm(wasm);
  const [, out] = w.call("orvyn_validate", typeof game === "string" ? game : JSON.stringify(game));
  return (out as { problems: string[] }).problems;
}

export class Player {
  private keys = new Set<string>();
  private touchKeys = new Set<string>();
  private pointer: [number, number] | null = null;
  private down = false;
  private images = new Map<string, HTMLImageElement>();
  private sounds = new Map<string, HTMLAudioElement>();
  private scripts: ScriptModule[] = [];
  private raf = 0;
  private acc = 0;
  private last = 0;
  private paused = false;
  private started = false;
  private ended = false;
  private overlay: HTMLDivElement | null = null;
  private cleanup: (() => void)[] = [];
  // Input recording (for leaderboards) and saves.
  private steps = 0;
  private log: InputLog = { v: 1, frames: 0, keys: [], pointer: [], start: {} };
  private prevKeys = new Set<string>();
  private prevPointer = "";
  private saved: Record<string, unknown> = {};
  private savedJson = "{}";
  private saveTimer = 0;
  readonly api: Api;
  last_frame: Frame | null = null;
  readonly width: number;
  readonly height: number;
  private ctx: CanvasRenderingContext2D;

  constructor(
    private w: Wasm,
    private game: Game,
    private opts: PlayOptions,
  ) {
    [this.width, this.height] = game.size ?? [480, 270];
    this.ctx = opts.canvas.getContext("2d")!;
    const api = (op: string, args: Record<string, unknown> = {}) => this.w.call("orvyn_api", JSON.stringify({ op, ...args }))[1];
    this.api = {
      spawn: (prefab, x, y) => api("spawn", { prefab, x, y }) as number | null,
      destroy: (id) => void api("destroy", { id }),
      entity: (id) => api("get_entity", { id }) as ReturnType<Api["entity"]>,
      setPos: (id, x, y) => void api("set_pos", { id, x, y }),
      setVel: (id, x, y) => void api("set_vel", { id, x, y }),
      find: (tag) => (api("find", { tag }) as number[]) ?? [],
      get: (name, id) => api("get", { name, id }),
      set: (name, value, id) => void api("set", { name, value, id }),
      random: () => api("random") as number,
      sound: (name) => void api("sound", { name }),
      scene: (name) => void api("scene", { name }),
      win: (message = "") => void api("win", { message }),
      lose: (message = "") => void api("lose", { message }),
      text: (id, text) => void api("text", { id, text }),
      key: (key) => api("key", { key }) as boolean,
      pressed: (key) => api("pressed", { key }) as boolean,
    };
  }

  private asset(path: string) {
    return this.opts.asset ? this.opts.asset(path) : new URL(path, location.href).href;
  }

  async prepare() {
    const a = this.game.assets ?? {};
    await Promise.all(
      Object.entries(a.images ?? {}).map(
        ([name, img]) =>
          new Promise<void>((resolve) => {
            const el = new Image();
            el.onload = () => resolve();
            el.onerror = () => (console.error(`Orvyn: image ${name} (${img.src}) didn't load`), resolve());
            el.src = img.src.startsWith("data:") ? img.src : this.asset(img.src);
            this.images.set(name, el);
          }),
      ),
    );
    for (const [name, src] of Object.entries(a.sounds ?? {})) {
      const el = new Audio(src.startsWith("data:") ? src : this.asset(src));
      el.preload = "auto";
      this.sounds.set(name, el);
    }
    // TS/JS scripts, compiled to JS before publishing: each is an ES module.
    for (const [name, src] of Object.entries(this.game.scripts ?? {})) {
      if (!/\.(js|ts)$/.test(name)) continue;
      const url = URL.createObjectURL(new Blob([src], { type: "text/javascript" }));
      try {
        this.scripts.push((await import(/* @vite-ignore */ url)) as ScriptModule);
      } catch (e) {
        throw new Error(`script ${name}: ${(e as Error).message}`);
      }
      URL.revokeObjectURL(url);
    }
  }

  private listen() {
    const c = this.opts.canvas;
    const on = <K extends keyof WindowEventMap>(t: EventTarget, type: K | string, fn: (e: never) => void, o?: AddEventListenerOptions) => {
      t.addEventListener(type, fn as EventListener, o);
      this.cleanup.push(() => t.removeEventListener(type, fn as EventListener));
    };
    on(window, "keydown", (e: KeyboardEvent) => {
      const k = keyName(e);
      if (!k) return;
      this.keys.add(k);
      if (["left", "right", "up", "down", "space"].includes(k)) e.preventDefault();
    });
    on(window, "keyup", (e: KeyboardEvent) => {
      const k = keyName(e);
      if (k) this.keys.delete(k);
    });
    on(window, "blur", () => this.keys.clear());
    const toGame = (e: PointerEvent): [number, number] => {
      const r = c.getBoundingClientRect();
      return [((e.clientX - r.left) / r.width) * this.width, ((e.clientY - r.top) / r.height) * this.height];
    };
    on(c, "pointerdown", (e: PointerEvent) => {
      this.pointer = toGame(e);
      this.down = true;
      c.setPointerCapture?.(e.pointerId);
    });
    on(c, "pointermove", (e: PointerEvent) => (this.pointer = toGame(e)));
    on(c, "pointerup", () => (this.down = false));
    on(c, "pointercancel", () => (this.down = false));
    c.style.touchAction = "none";
  }

  /** On-screen buttons for touch screens (the keys the game asks for). */
  private touchControls() {
    const t = this.game.touch;
    const want = this.opts.touchControls ?? (matchMedia("(pointer: coarse)").matches && !!(t?.pad || t?.buttons?.length));
    if (!want || !t) return;
    const host = this.opts.canvas.parentElement ?? document.body;
    const bar = document.createElement("div");
    bar.className = "orvyn-touch";
    Object.assign(bar.style, { position: "absolute", left: "0", right: "0", bottom: "12px", display: "flex", justifyContent: "space-between", padding: "0 12px", pointerEvents: "none", userSelect: "none" });
    const group = () => Object.assign(document.createElement("div"), { style: "display:flex;gap:10px;pointer-events:auto" });
    const button = (label: string, key: string) => {
      const b = document.createElement("button");
      b.textContent = label;
      b.setAttribute("aria-label", key);
      Object.assign(b.style, { width: "56px", height: "56px", borderRadius: "50%", border: "1px solid #ffffff55", background: "#ffffff22", color: "#fff", font: "600 18px system-ui", touchAction: "none" });
      const press = (on: boolean) => (on ? this.touchKeys.add(key) : this.touchKeys.delete(key));
      b.addEventListener("pointerdown", (e) => (e.preventDefault(), press(true)));
      for (const ev of ["pointerup", "pointercancel", "pointerleave"]) b.addEventListener(ev, () => press(false));
      return b;
    };
    const left = group();
    if (t.pad === "dpad" || t.pad === "leftright") left.append(button("◀", "left"), button("▶", "right"));
    if (t.pad === "dpad") left.append(button("▲", "up"), button("▼", "down"));
    const right = group();
    (t.buttons ?? []).forEach((k, i) => right.append(button(k === "space" ? "A" : String.fromCharCode(66 + i - 1).replace("@", "A"), k)));
    bar.append(left, right);
    if (getComputedStyle(host).position === "static") host.style.position = "relative";
    host.append(bar);
    this.cleanup.push(() => bar.remove());
  }

  private resize() {
    const c = this.opts.canvas;
    const dpr = Math.min(window.devicePixelRatio || 1, 3);
    const box = c.parentElement?.getBoundingClientRect() ?? { width: window.innerWidth, height: window.innerHeight };
    const scale = Math.max(0.1, Math.min(box.width / this.width, (box.height || window.innerHeight) / this.height));
    c.style.width = `${Math.floor(this.width * scale)}px`;
    c.style.height = `${Math.floor(this.height * scale)}px`;
    c.width = Math.round(this.width * scale * dpr);
    c.height = Math.round(this.height * scale * dpr);
    this.ctx.setTransform(c.width / this.width, 0, 0, c.height / this.height, 0, 0);
    this.ctx.imageSmoothingEnabled = false;
    if (this.last_frame) this.draw(this.last_frame);
  }

  /** Advance one fixed step (1/60 s); public for tests and frame-by-frame tools. */
  step(): Frame {
    const keys = [...new Set([...this.keys, ...this.touchKeys])].sort();
    // Whole pixels: what the engine gets is exactly what's recorded, so a replay matches.
    const pointer: [number, number] | null = this.pointer ? [Math.round(this.pointer[0]), Math.round(this.pointer[1])] : null;
    const n = this.steps++;
    const now = new Set(keys);
    for (const k of now) if (!this.prevKeys.has(k)) this.log.keys.push([n, `+${k}`]);
    for (const k of this.prevKeys) if (!now.has(k)) this.log.keys.push([n, `-${k}`]);
    this.prevKeys = now;
    const p = `${pointer?.[0] ?? ""},${pointer?.[1] ?? ""},${this.down}`;
    if (p !== this.prevPointer) {
      this.log.pointer.push([n, pointer?.[0] ?? null, pointer?.[1] ?? null, this.down]);
      this.prevPointer = p;
    }
    this.log.frames = this.steps;
    const [, f] = this.w.call("orvyn_step", JSON.stringify({ keys, pointer, down: this.down }));
    const frame = f as Frame;
    const api = this.api;
    try {
      if (!this.started) {
        this.started = true;
        for (const m of this.scripts) m.onStart?.(api);
      }
      for (const m of this.scripts) m.onTick?.(api, 1 / 60);
      for (const [a, b] of frame.contacts) for (const m of this.scripts) m.onCollide?.(api, a, b);
      for (const call of frame.calls) {
        const fn = this.scripts.map((m) => m[call.function]).find((x) => typeof x === "function") as ((api: Api, a: number | null, b: number | null) => void) | undefined;
        if (fn) fn(api, call.a, call.b);
        else frame.errors.push(`a rule calls ${call.function}, but no script defines it`);
      }
    } catch (e) {
      frame.halted = `script: ${(e as Error).message}`;
    }
    for (const s of frame.sounds) {
      const el = this.sounds.get(s);
      if (el) {
        const copy = el.cloneNode(true) as HTMLAudioElement;
        void copy.play().catch(() => undefined);
      }
    }
    for (const l of frame.logs) console.log(`[${this.game.title}]`, l);
    this.watchSave(frame);
    this.last_frame = frame;
    return frame;
  }

  /** The game's saved variables changed: hand them on (debounced). */
  private watchSave(frame: Frame) {
    const keys = this.game.save ?? [];
    if (!keys.length || !this.opts.save?.onChange) return;
    const data = Object.fromEntries(keys.filter((k) => k in frame.vars).map((k) => [k, frame.vars[k]]));
    const json = JSON.stringify(data);
    if (json === this.savedJson) return;
    this.savedJson = json;
    clearTimeout(this.saveTimer);
    this.saveTimer = setTimeout(() => this.opts.save?.onChange?.(data), 700) as unknown as number;
  }

  /** Stored: the next run starts from these saved values. */
  confirmSave(data: Record<string, unknown>) {
    this.saved = { ...data };
  }

  /** Set the saved variables into a freshly loaded engine, and start a new input log. */
  private beginRun() {
    const keys = this.game.save ?? [];
    const start = Object.fromEntries(Object.entries(this.saved).filter(([k]) => keys.includes(k)));
    for (const [name, value] of Object.entries(start)) this.w.call("orvyn_api", JSON.stringify({ op: "set", name, value }));
    this.steps = 0;
    this.log = { v: 1, frames: 0, keys: [], pointer: [], start };
    this.prevKeys = new Set();
    this.prevPointer = "";
    this.savedJson = JSON.stringify(start);
  }

  draw(f: Frame) {
    const ctx = this.ctx;
    ctx.globalAlpha = 1;
    ctx.fillStyle = f.background;
    ctx.fillRect(0, 0, this.width, this.height);
    for (const d of f.draws) {
      ctx.globalAlpha = d.a;
      switch (d.k) {
        case "r":
          ctx.fillStyle = d.c;
          if (d.rot) {
            ctx.save();
            ctx.translate(d.x + d.w / 2, d.y + d.h / 2);
            ctx.rotate((d.rot * Math.PI) / 180);
            ctx.fillRect(-d.w / 2, -d.h / 2, d.w, d.h);
            ctx.restore();
          } else ctx.fillRect(d.x, d.y, d.w, d.h);
          break;
        case "o":
          ctx.fillStyle = d.c;
          ctx.beginPath();
          ctx.ellipse(d.x + d.w / 2, d.y + d.h / 2, d.w / 2, d.h / 2, 0, 0, Math.PI * 2);
          ctx.fill();
          break;
        case "i": {
          const img = this.images.get(d.img);
          if (!img || !img.complete || !img.naturalWidth) break;
          let [sx, sy, sw, sh] = [0, 0, img.naturalWidth, img.naturalHeight];
          if (d.fw > 0 && d.fh > 0) {
            const cols = Math.max(1, Math.floor(img.naturalWidth / d.fw));
            [sx, sy, sw, sh] = [(d.f % cols) * d.fw, Math.floor(d.f / cols) * d.fh, d.fw, d.fh];
          }
          ctx.save();
          ctx.translate(d.x + d.w / 2, d.y + d.h / 2);
          if (d.rot) ctx.rotate((d.rot * Math.PI) / 180);
          if (d.flip) ctx.scale(-1, 1);
          ctx.drawImage(img, sx, sy, sw, sh, -d.w / 2, -d.h / 2, d.w, d.h);
          ctx.restore();
          break;
        }
        case "t": {
          ctx.fillStyle = d.c;
          ctx.font = `600 ${d.size}px system-ui, sans-serif`;
          ctx.textBaseline = "top";
          ctx.textAlign = (d.align as CanvasTextAlign) ?? "center";
          const x = d.align === "left" ? d.x : d.align === "right" ? d.x + d.w : d.x + d.w / 2;
          ctx.fillText(d.s, x, d.y);
          break;
        }
      }
    }
    ctx.globalAlpha = 1;
  }

  private showOverlay(title: string, body: string | null, action?: { label: string; run: () => void }) {
    this.hideOverlay();
    const host = this.opts.canvas.parentElement ?? document.body;
    const o = document.createElement("div");
    o.className = "orvyn-overlay";
    Object.assign(o.style, { position: "absolute", inset: "0", display: "grid", placeContent: "center", gap: "12px", textAlign: "center", background: "#000a", color: "#fff", font: "500 16px system-ui", padding: "16px" });
    const h = Object.assign(document.createElement("div"), { textContent: title });
    h.style.font = "700 24px system-ui";
    o.append(h);
    if (body) o.append(Object.assign(document.createElement("div"), { textContent: body }));
    if (action) {
      const b = Object.assign(document.createElement("button"), { textContent: action.label });
      Object.assign(b.style, { justifySelf: "center", padding: "10px 18px", borderRadius: "999px", border: "0", background: "#fff", color: "#000", font: "600 15px system-ui", cursor: "pointer" });
      b.onclick = action.run;
      o.append(b);
    }
    if (getComputedStyle(host).position === "static") host.style.position = "relative";
    host.append(o);
    this.overlay = o;
  }

  private hideOverlay() {
    this.overlay?.remove();
    this.overlay = null;
  }

  private loop = (now: number) => {
    this.raf = requestAnimationFrame(this.loop);
    if (this.paused || this.ended) {
      this.last = now;
      return;
    }
    this.acc += Math.min(0.25, (now - (this.last || now)) / 1000);
    this.last = now;
    let f: Frame | null = null;
    let steps = 0;
    while (this.acc >= 1 / 60 && steps < 5) {
      f = this.step();
      this.acc -= 1 / 60;
      steps++;
      if (f.status !== "running" || f.halted) break;
    }
    if (!f) return;
    this.draw(f);
    this.opts.onFrame?.(f);
    if (f.halted) {
      this.ended = true;
      this.opts.onError?.(f.halted);
      this.showOverlay("The game hit an error", f.halted);
    } else if (f.status !== "running") {
      this.ended = true;
      this.opts.onEnd?.(f.status, f.message);
      this.opts.onRun?.({ status: f.status, message: f.message, time: f.time, vars: f.vars, log: { ...this.log, keys: [...this.log.keys], pointer: [...this.log.pointer] } });
      this.showOverlay(f.status === "won" ? "You win!" : "Game over", f.message, { label: "Play again", run: () => this.restart() });
    }
  };

  start() {
    this.saved = { ...(this.opts.save?.data ?? {}) };
    this.beginRun();
    this.listen();
    this.touchControls();
    const ro = new ResizeObserver(() => this.resize());
    ro.observe(this.opts.canvas.parentElement ?? document.body);
    this.cleanup.push(() => ro.disconnect());
    this.resize();
    // Draw the first frame now, so the canvas is never blank while the loop warms up.
    this.draw(this.step());
    this.raf = requestAnimationFrame(this.loop);
  }

  /** Hold these keys (as if pressed) until changed: for replays, demos and thumbnails. */
  hold(keys: string[]) {
    this.keys = new Set(keys.map((k) => k.toLowerCase()));
  }

  /**
   * Pause real time and play `steps` frames holding `keys`, then draw: deterministic, so the same
   * replay always ends on the same picture (thumbnails, tests).
   */
  simulate(steps: number, keys: string[] = []) {
    this.pause();
    this.hold(keys);
    let f = this.last_frame;
    for (let i = 0; i < steps; i++) f = this.step();
    this.hold([]);
    if (f) this.draw(f);
    return f;
  }

  pause() {
    this.paused = true;
  }

  resume() {
    this.paused = false;
  }

  /** Start over from the game file. */
  restart() {
    const [code, out] = this.w.call("orvyn_load", JSON.stringify(this.game));
    if (code !== 0) throw new Error((out as { problems: string[] }).problems.join("\n"));
    this.started = false;
    this.ended = false;
    this.acc = 0;
    this.beginRun();
    this.hideOverlay();
  }

  destroy() {
    cancelAnimationFrame(this.raf);
    for (const c of this.cleanup) c();
    this.hideOverlay();
  }
}

/** Load the engine and a game, and start playing. */
export async function play(opts: PlayOptions): Promise<Player> {
  const game: Game = typeof opts.game === "string" ? JSON.parse(opts.game) : opts.game;
  const w = await loadWasm(opts.wasm);
  const [code, out] = w.call("orvyn_load", JSON.stringify(game));
  if (code !== 0) throw new Error(`This game can't start:\n${(out as { problems: string[] }).problems.join("\n")}`);
  const p = new Player(w, game, opts);
  await p.prepare();
  p.start();
  return p;
}
