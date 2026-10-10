# 开发与贡献

欢迎提交问题、设备兼容性反馈和小范围改进。先阅读 [README](README.md) 的适用范围：当前主要面向 Windows + macOS、MX Keys 与 Logitech 接收器，不承诺所有键鼠均可使用。

## 目录职责

| 目录 / 文件 | 用途 |
| --- | --- |
| `src/` | React 界面、设置与前后端接口；界面测试就近放置 |
| `src-tauri/src/` | Rust：输入判断、设备控制、消息协议、双机运行状态与桌面入口 |
| `src-tauri/native/` | macOS / Windows 原生输入桥接与 Mac 接收器访问 |
| `src-tauri/icons/` | 桌面打包使用的图标及图标源文件，不维护手机或商店资源 |
| `public/` | 应用静态资源；README 与应用复用同一品牌图标 |
| `scripts/` | 本地检查、Mac 构建签名与 Windows 诊断 / 修复工具 |
| `script/build_and_run.sh` | Mac 的 Codex Run 入口，复用 `scripts/` 的构建签名流程 |
| `docs/` | 架构、设计资料、历史检查记录与第三方许可材料 |
| `.github/` | 问题模板和历史 workflow；Actions 当前停用 |

`node_modules/`、`dist/`、`src-tauri/target/`、`.artifacts/` 是本机依赖、构建产物或交付证据，不进入源码仓库。不要把删除测试、许可证或锁文件当作瘦身。

## 构建环境

- Node.js：24.15 或以上的 24.x，或 22.22.2 或以上的 22.x；完整范围见 `package.json`。
- Rust stable，支持 Rust 2024 edition。
- Windows：Visual Studio Build Tools 的 C++ 桌面开发工具、Windows SDK、WebView2。
- macOS：Xcode Command Line Tools。

系统依赖见 [Tauri 官方说明](https://v2.tauri.app/start/prerequisites/)。

```bash
git clone https://github.com/s840207702/KeyClaim.git
cd KeyClaim
npm ci
npm run tauri dev
```

`npm run dev` 只提供浏览器示例状态，不控制键盘。实机开发使用 `npm run tauri dev`。

## 本地检查

安装对应平台依赖后，在项目根目录运行：

```bash
npm run check
```

检查依次覆盖：五处版本字段一致性、前端测试、TypeScript 与前端构建、Rust 格式及锁定依赖下的 Rust 测试；Windows 另做 PowerShell 脚本语法检查，不执行诊断或修复脚本。任何一步失败都会停止；不会安装软件或改配置、防火墙。

也可单独运行现有命令：

```bash
npm test
npm run build
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo test --manifest-path src-tauri/Cargo.toml --locked
npm audit --registry=https://registry.npmjs.org
```

单元测试和构建通过不等于实机切换通过。真实接收器测试默认忽略；涉及切换的改动必须额外检查安装后的唯一进程、UDP/TCP 端口归属、输入监听、握手和键盘真正到达目标通道。

## 本地打包

### Windows

```powershell
npm run build:windows
```

NSIS 安装包位于 `src-tauri/target/release/bundle/nsis/`。构建不会自动更新已安装的软件。

### macOS

```bash
npm run build:mac
```

App 位于 `src-tauri/target/release/bundle/macos/KeyClaim.app`。没有 `APPLE_SIGNING_IDENTITY` 时使用项目原有的稳定本地 ad-hoc 签名；设置有效签名身份时保留 Tauri 的签名结果。

需要构建并运行开发产物时使用已有 Codex Run 入口：

```bash
./script/build_and_run.sh --verify
```

支持 `--debug`、`--logs`、`--telemetry`；此入口会停止旧 KeyClaim，并启动仓库里的开发产物，**不是更新 `/Applications` 正式安装版**。

若使用 Tauri 直接生成 DMG：

```bash
npm run build:desktop -- --bundles app,dmg
```

此命令会重新构建，不应假定 DMG 中的 App 继承此前本地脚本的签名。正式分发按 [Tauri macOS 签名说明](https://v2.tauri.app/distribute/sign/macos/) 处理，并检查交付 DMG 内实际应用的签名、公证与版本。ad-hoc 不是 Developer ID 签名或公证。

发布前应核对源码提交、版本、安装包 SHA256、签名状态，并用交付包安装验证；保留用户配置和旧应用备份。不要只以构建成功作为验收结果。

## 报告问题

在 [Issues](https://github.com/s840207702/KeyClaim/issues) 提供版本、系统与架构、键盘及接收器型号、接收器 / 蓝牙连接方式、复现步骤和问题时间。

可以附脱敏日志，但不要上传完整配置、配对码或未经检查的诊断 JSON。配对码指纹、主机名、IP、序列号和本机完整路径也应删除或替换为占位符。安全问题按 [SECURITY.md](SECURITY.md) 私下报告。

## 提交改动

1. Fork 仓库并创建工作分支，每次只解决一个问题。
2. 不附带无关重构或整仓格式化；保留核心功能、测试、锁文件和许可材料。
3. 不提交配置、真实日志、密钥、证书或安装包。
4. PR 写明原因、影响、检查结果，以及哪些硬件和双机场景真正测过。

维护者选择本地构建与手动发布，不要求贡献者启用付费服务。历史 workflow 保留作参考；Actions 继续停用。

贡献者应有权提供改动，并同意按项目 MIT 许可证发布；引用第三方代码须保留相应版权和许可声明。
