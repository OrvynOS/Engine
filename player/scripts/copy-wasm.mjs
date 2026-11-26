// Copy the release build of the engine next to the player bundle.
import { copyFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
const here = dirname(fileURLToPath(import.meta.url));
const src = join(here, "..", "..", "target", "wasm32-unknown-unknown", "release", "orvyn_engine.wasm");
mkdirSync(join(here, "..", "dist"), { recursive: true });
copyFileSync(src, join(here, "..", "dist", "orvyn_engine.wasm"));
console.log("dist/orvyn_engine.wasm");
