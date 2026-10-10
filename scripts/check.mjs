import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

const root = fileURLToPath(new URL("../", import.meta.url));
const read = (path) => readFileSync(new URL(`../${path}`, import.meta.url), "utf8");
const metadata = JSON.parse(read("package.json"));
const lock = JSON.parse(read("package-lock.json"));
const cargo = read("src-tauri/Cargo.toml");
const cargoLock = read("src-tauri/Cargo.lock");
const versions = [
  ["package-lock.json", lock.version],
  ["package-lock.json packages[\"\"]", lock.packages?.[""]?.version],
  ["src-tauri/tauri.conf.json", JSON.parse(read("src-tauri/tauri.conf.json")).version],
  ["src-tauri/Cargo.toml", cargo.match(/\[package\][\s\S]*?^version\s*=\s*"([^"]+)"/m)?.[1]],
  ["src-tauri/Cargo.lock", cargoLock.match(/\[\[package\]\]\s*name = "keyclaim"\s*version = "([^"]+)"/)?.[1]],
];

for (const [path, version] of versions) {
  if (version !== metadata.version) {
    console.error(`版本不一致：${path} 是 ${version ?? "未找到"}，package.json 是 ${metadata.version}`);
    process.exit(1);
  }
}
console.log(`版本一致性通过：${metadata.version}`);

// 使用 npm 提供的入口和同一个 Node，避免 Windows .cmd 与 shell 转义差异。
const npmCli = process.env.npm_execpath;
if (!npmCli) {
  console.error("请通过 npm run check 执行完整检查。");
  process.exit(1);
}

const steps = [
  ["前端测试", process.execPath, [npmCli, "test"]],
  ["前端构建", process.execPath, [npmCli, "run", "build"]],
  ["Rust 格式", "cargo", ["fmt", "--manifest-path", "src-tauri/Cargo.toml", "--", "--check"]],
  ["Rust 测试", "cargo", ["test", "--manifest-path", "src-tauri/Cargo.toml", "--locked"]],
];

for (const [label, command, args] of steps) {
  console.log(`\n检查：${label}`);
  const result = spawnSync(command, args, { cwd: root, stdio: "inherit" });
  if (result.error) {
    console.error(`${label} 无法启动：${result.error.message}`);
    process.exit(1);
  }
  if (result.status !== 0) process.exit(result.status ?? 1);
}

if (process.platform === "win32") {
  console.log("\n检查：PowerShell 脚本语法（不执行脚本）");
  const result = spawnSync("powershell.exe", [
    "-NoProfile", "-Command",
    "$ErrorActionPreference='Stop'; Get-ChildItem -LiteralPath scripts -Filter '*.ps1' | ForEach-Object { [void][scriptblock]::Create((Get-Content -LiteralPath $_.FullName -Raw -Encoding UTF8)) }",
  ], { cwd: root, stdio: "inherit" });
  if (result.error) {
    console.error(result.error.message);
    process.exit(1);
  }
  if (result.status !== 0) process.exit(result.status ?? 1);
}

console.log("\n全部本地检查通过。未安装软件、未修改配置或防火墙。");
