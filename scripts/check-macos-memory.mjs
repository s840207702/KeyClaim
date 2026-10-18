import { mkdtempSync, rmSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

if (process.platform !== "darwin") {
  console.error("此硬件内存回归检查仅适用于 macOS。");
  process.exit(77);
}

const root = fileURLToPath(new URL("../", import.meta.url));
const temporary = mkdtempSync(path.join(os.tmpdir(), "keyclaim-memory-"));
const executable = path.join(temporary, "check-macos-memory");
let status = 1;
try {
  const build = spawnSync("xcrun", [
    "clang", "-O2", "-Wall", "-Wextra", "-Werror", "-Wno-deprecated-declarations",
    "scripts/check-macos-memory.m", "-framework", "IOKit", "-framework", "CoreFoundation",
    "-lobjc", "-o", executable,
  ], { cwd: root, stdio: "inherit" });
  if (build.error) throw build.error;
  status = build.status ?? 1;
  if (status === 0) {
    const check = spawnSync(executable, [], { cwd: root, stdio: "inherit" });
    if (check.error) throw check.error;
    status = check.status ?? 1;
  }
} finally {
  rmSync(temporary, { recursive: true, force: true });
}
process.exit(status);
