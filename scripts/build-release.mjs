import os from "node:os";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

const root = fileURLToPath(new URL("../", import.meta.url)).replace(/[\\/]$/, "");
const cli = fileURLToPath(new URL("../node_modules/@tauri-apps/cli/tauri.js", import.meta.url));
const flags = process.env.CARGO_ENCODED_RUSTFLAGS?.split("\x1f").filter(Boolean) ?? [];

if (!flags.length && process.env.RUSTFLAGS?.trim()) {
  console.error("请通过 CARGO_ENCODED_RUSTFLAGS 传入额外 Rust 参数。");
  process.exit(1);
}

for (const [source, destination] of [[os.homedir(), "/build/user"], [root, "/build/KeyClaim"]]) {
  for (const prefix of new Set([source, source.replaceAll("\\", "/")])) {
    flags.push(`--remap-path-prefix=${prefix}=${destination}`);
  }
}
if (process.platform === "win32") {
  flags.push("-C", "link-arg=/PDBALTPATH:%_PDB%");
}

const result = spawnSync(process.execPath, [cli, "build", ...process.argv.slice(2)], {
  cwd: root,
  stdio: "inherit",
  env: { ...process.env, RUSTFLAGS: "", CARGO_ENCODED_RUSTFLAGS: flags.join("\x1f") },
});
if (result.error) {
  console.error(result.error.message);
  process.exit(1);
}
process.exit(result.status ?? 1);
