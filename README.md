<div align="center">
  <img src="docs/assets/keyclaim-icon.png" width="156" alt="KeyClaim · 键来图标">
  <h1>KeyClaim · 键来</h1>
  <p>让 MX Keys 自动跟随你正在使用的 Windows 或 Mac。</p>
  <p><a href="https://github.com/s840207702/KeyClaim/releases/latest">下载安装包</a> · <a href="docs/ARCHITECTURE.md">实现原理</a> · <a href="CONTRIBUTING.md">参与项目</a> · <a href="LICENSE">MIT 许可证</a></p>
</div>

![KeyClaim 主界面示例](docs/assets/keyclaim-home.png)

KeyClaim 是为双机办公开发的本地桌面工具：两台电脑各自使用自己的鼠标或触控板，共用一把 MX Keys。你在另一台电脑上操作鼠标时，当前持有键盘的电脑通过 Logitech HID++ 将键盘切到目标通道。

它**不是网络鼠标键盘转发软件**，不把鼠标指针、按键内容或剪贴板从一台电脑传到另一台；也不是 Logitech 官方产品，与 Logitech 没有隶属或赞助关系。

## 适用范围

项目从一套实际设备组合发展而来，目前仍属于早期版本，不是通用的所有品牌键鼠切换工具。

| 项目 | 已使用的组合与范围 |
| --- | --- |
| 键盘 | MX Keys，通过支持 HID++ ChangeHost 的接收器分别连接两台电脑 |
| Windows | x64；本机鼠标为 MX Master 3 |
| macOS | Apple Silicon；本机鼠标为 MX Master 3S，也使用内置触控板 |
| 键盘通道 | 默认 Windows 为 2、Mac 为 3，可在设置中调整 |
| 网络 | 两端处于可信的同一局域网，保持运行并可处理后台任务 |

鼠标仅用于检测本机输入，不需要具备 MX 系列切换能力。其他鼠标可能适用，但未逐一验证。其他键盘、MX Keys S、Bolt 键盘、纯蓝牙连接、Intel Mac、Linux 或三台电脑同时协作均不在当前实机验收范围内。

macOS 接收器探测优先使用 PID `0xC52B` 的 Unifying 接收器，并检查设备类型，避免把 PID `0xC548` 的 Bolt 鼠标当作键盘。

## 下载安装

从 [GitHub Releases](https://github.com/s840207702/KeyClaim/releases) 下载对应安装包，不需要 Node.js 或 Rust：

| 平台 | 0.2.6 安装包 | 安装方法 |
| --- | --- | --- |
| Windows x64 | `KeyClaim_0.2.6_x64-setup.exe` | 退出旧版后运行安装向导 |
| macOS Apple Silicon | `KeyClaim_0.2.6_aarch64.dmg` | 打开 DMG，将 KeyClaim.app 拖入 Applications |

Windows 面向 Windows 10 / 11，运行需要 Microsoft Edge WebView2 Runtime。macOS 的完整最低系统版本兼容性尚未逐一验证；本次只发布 Apple Silicon 包。

当前 Windows 包未签名，macOS 包未经过 Apple 公证，系统可能提示或阻止打开。请先核对 Release 提供的 `SHA256SUMS.txt`，只使用你信任的来源；不要关闭 SmartScreen、Gatekeeper 或其他系统安全保护。

### 首次配对

1. 先用键盘的 Easy-Switch 将两台电脑分别配对到两个不同通道，确认手动切换正常。
2. 在两台电脑上安装并启动 KeyClaim。
3. 打开设置（齿轮按钮、`Ctrl+,` 或 `⌘,`），设置各自的本机通道。默认 Windows 为 2，Mac 为 3；使用其他通道时，两端的本机和对端配置必须互相对应。
4. 在两端填入**相同的自定义随机配对码**，建议不少于 32 个随机字符，并保存。不要保留源码中的公共默认值，也不要使用账号密码。
5. 在 macOS 的“系统设置 → 隐私与安全性 → 输入监控”中允许 KeyClaim；按系统提示允许本地网络访问。
6. 确认两端就绪，开启“自动跟随”，在另一台电脑移动鼠标或触控板，检查键盘是否真正归位。

Windows 关闭主窗口后仍驻留托盘；通过托盘菜单退出才能结束服务。macOS 默认只驻留菜单栏，不占 Dock；点击图标打开面板，通过设置页“退出键来”结束。两端可在界面中开启开机自动运行。

配对码是局域网消息的校验凭据，不是云账号或蓝牙配对码；KeyClaim 不会替你完成键盘与接收器的硬件配对。

## 工作方式

```text
目标电脑检测到本地鼠标 / 触控板活动
                    ↓
通过局域网发送带签名的召回请求
                    ↓
当前持有键盘的电脑发送 HID++ ChangeHost
                    ↓
目标电脑探测到 MX Keys 上线后确认成功
```

- macOS 使用原生 CGEventTap，Windows 使用 Raw Input；只检测本地指针活动，不记录按键内容。
- 持久 TCP 是主链路，UDP 广播辅助发现；默认 TCP/UDP 端口均为 `45823`。Windows 会在本地 `/24` 网段探测对端。
- 消息使用 HMAC-SHA256 校验，并进行时效、重复请求与通道检查；**通信不加密**。
- 当前持有键盘的电脑必须在线并能处理请求。持有方深度休眠时，仍需使用键盘的 Easy-Switch 手动切换。
- 应用使用单实例保护；真实到达确认与“已发出切换指令”分开处理。

详细说明见 [技术架构](docs/ARCHITECTURE.md) 和 [设计规格](docs/design/SPEC.md)。

## 排查问题

先确认手动 Easy-Switch 可用、两端配对码一致、通道互相对应、键盘已唤醒，并检查接收器和系统权限。关闭“自动跟随”即可停止自动召回。

Windows 在源码项目目录可运行只读诊断：

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\diagnose-windows.ps1
```

脚本读取进程、UDP 监听、防火墙、网卡、配置摘要和 Logitech HID 设备，不修改系统设置，也不输出配对码明文。不过它会输出配对码指纹、主机名、IP 和设备标识，公开反馈前仍需脱敏；它不检查 TCP 监听。

`scripts/repair-windows-network.ps1` 是可选的**写操作**：请求管理员权限，创建所有网络配置文件下的本地子网 UDP 45823 入站规则，并重启应用。它不是通用修复，不处理 TCP 或出站规则；只有确认需要该规则时才使用。不要为排查问题关闭整个防火墙。

## 开发与本地构建

### 前置依赖

- 推荐 Node.js 24.15 或以上的 24.x；也可使用 22.22.2 或以上的 22.x。具体支持范围见 `package.json`，不是所有 Node.js 20 都能运行当前依赖。
- Rust stable，支持 Rust 2024 edition。
- Windows：Visual Studio Build Tools 的“使用 C++ 的桌面开发”、Windows SDK 与 WebView2。
- macOS：Xcode Command Line Tools。

系统依赖安装步骤见 [Tauri 官方文档](https://v2.tauri.app/start/prerequisites/)。克隆后在项目根目录执行：

```bash
git clone https://github.com/s840207702/KeyClaim.git
cd KeyClaim
npm ci
npm run tauri dev
```

`npm run dev` 只启动浏览器界面预览，使用示例状态，不会实际控制键盘；实机开发应使用 `npm run tauri dev`。

### Windows 安装包

```powershell
npm run tauri build -- --bundles nsis
```

产物位于 `src-tauri/target/release/bundle/nsis/`。本机构建不会自动安装，也不会更新已安装的应用。

### macOS 应用与 DMG

```bash
npm run build:mac
npm run tauri build -- --bundles app,dmg
```

本地签名的 App 位于 `src-tauri/target/release/bundle/macos/KeyClaim.app`；DMG 位于 `src-tauri/target/release/bundle/dmg/`。注意第二条命令会重新构建，DMG 内的 App 不应视为继承第一条命令的签名。正式分发需要按 [Tauri macOS 签名文档](https://v2.tauri.app/distribute/sign/macos/) 签名、公证，再核对 DMG 内的实际应用。`build:mac` 只做稳定身份的本地 ad-hoc 签名，不等于 Developer ID 签名或公证。

### 检查

```bash
npm test
npm run build
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo test --manifest-path src-tauri/Cargo.toml --locked
npm audit --registry=https://registry.npmjs.org
```

现有单元测试覆盖界面、消息协议、通道与召回状态等。真实 Logitech 接收器测试默认忽略，单元测试或构建成功不等于双机切换验收。

本仓库保留历史 workflow 文件，但 GitHub Actions 当前停用；发布包通过本机手动构建和上传，不会因提交代码自动运行构建。

## 当前状态与已知限制

当前版本为 `0.2.6`：

- 发布包已在现有 Windows 和 Apple Silicon Mac 上实际安装、启动，验证唯一进程、TCP/UDP 端口和局域网握手。
- Windows 有真实键盘归位成功记录，也有召回超时和接收器未找到记录。
- 新打包的 Mac 应用已验证输入监听与握手，尚未完成新包完整切换和长期稳定性验收。
- 不承诺所有接收器、所有系统版本、休眠恢复或快速连续切换均正常。
- Windows 未签名；Mac 未公证；不提供 Intel Mac 安装包。

安装包验证详情见 [0.2.6 Release](https://github.com/s840207702/KeyClaim/releases/tag/v0.2.6)，开源准备检查见 [开源检查记录](docs/OPEN_SOURCE_READINESS.md)。

## 隐私、安全与贡献

配置、配对码和诊断日志保存在本机；没有账号、遥测或公网中继。配对码以明文保存，网络消息不加密，只应在可信局域网使用。公开反馈前删除配对码、配对码指纹和个人环境信息。详情与私密漏洞报告入口见 [安全说明](SECURITY.md)。

普通问题请提交 [Issue](https://github.com/s840207702/KeyClaim/issues)。参与开发见 [CONTRIBUTING.md](CONTRIBUTING.md)。

## 许可证与致谢

KeyClaim 自有代码按 [MIT License](LICENSE) 开源，允许修改、分发和商用，须保留版权及许可声明，不提供任何担保。

HID++ ChangeHost 与部分 macOS HID 报告处理参考 [mxswitch](https://github.com/marcocosta97/mxswitch)；第三方代码和依赖保留各自许可证，详见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。Logitech、MX Keys、MX Master、Unifying 和 Bolt 为其各自权利人的名称或商标。
