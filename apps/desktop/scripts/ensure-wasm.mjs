// Baut den WASM-Renderer, falls er noch fehlt (für `npm run dev`).
import { existsSync } from "node:fs";
import { execSync } from "node:child_process";

if (!existsSync(new URL("../src/lib/wasm/fmshot_render.js", import.meta.url))) {
  execSync("bash ../../scripts/build-wasm.sh", { stdio: "inherit" });
}
