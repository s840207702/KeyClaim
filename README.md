<div align="center">
  <img src="public/keyclaim-icon.png" width="156" alt="KeyClaim · 键来图标">
  <h1>KeyClaim · 键来</h1>
  <p>两台电脑各用自己的鼠标，一把 MX Keys 自动跟随你正在使用的 Windows 或 Mac。</p>
  <p><a href="https://github.com/s840207702/KeyClaim/releases">下载安装包</a> · <a href="docs/ARCHITECTURE.md">实现原理</a> · <a href="CONTRIBUTING.md">开发与贡献</a> · <a href="LICENSE">MIT 许可证</a></p>
</div>

![KeyClaim 通道与常用开关界面示例](docs/assets/keyclaim-home.png)

KeyClaim 检测本机鼠标或触控板活动，通过局域网通知当前持有键盘的电脑，让它把 MX Keys 切到目标通道。**它不转发鼠标指针、按键内容或剪贴板**，也不是 Logitech 官方产品。

## 下载安装

从 [GitHub Releases](https://github.com/s840207702/KeyClaim/releases) 下载，不需要安装 Node.js 或 Rust。

| 平台 | 当前公开安装包 | 安装方法 |
| --- | --- | --- |
| Windows 10 / 11 x64 | `KeyClaim_0.2.7_x64-setup.exe` | 退出旧版后运行安装向导；需要 WebView2 Runtime |
| macOS Apple Silicon | `KeyClaim_0.2.6_aarch64.dmg` | 打开 DMG，将 KeyClaim.app 拖入 Applications |

最新源码为 0.2.7；Mac 本机已从源码更新到 0.2.7，但尚未发布对应 DMG。0.2.7 没有改变通信协议，Windows 0.2.7 可与 Mac 0.2.6 配合使用。macOS 的完整最低系统版本兼容性尚未逐一验证。

Windows 安装包未签名，Mac 包未经过 Apple 公证，系统可能提示或阻止打开。请核对 Release 的 `SHA256SUMS.txt`，并一并保留许可证附件；不要关闭 SmartScreen、Gatekeeper 等系统安全保护。

## 适合哪些设备

| 项目 | 当前实机使用范围 |
| --- | --- |
| 键盘 | MX Keys，通过支持切换主机功能的 Logitech 接收器分别连接两台电脑 |
| 系统 | Windows x64 + macOS Apple Silicon |
| 鼠标 | Windows 使用 MX Master 3，Mac 使用 MX Master 3S 或内置触控板 |
| 通道 | 默认 Windows 为 2，Mac 为 3 |
| 网络 | 可信的同一局域网，两端在线并可处理后台任务 |

鼠标只用于检测本地活动，不需要具备 MX 系列切换能力；其他鼠标可能适用，但未逐一验证。其他键盘、MX Keys S、Bolt 键盘、纯蓝牙、Intel Mac、Linux 和三台电脑协作不在当前实机验收范围内。

## 首次配对

1. 先用键盘的 Easy-Switch 将两台电脑配到不同通道，确认手动切换正常。
2. 两端安装并启动 KeyClaim，点击右上角齿轮进入设置。
3. 使用默认 Windows 通道 2、Mac 通道 3；在两端填写相同的自定义随机配对码，建议不少于 32 个随机字符。不要使用公共默认值或账号密码。
4. 按系统提示允许 Mac 输入监控、本地网络访问，并确认系统允许两端局域网通信。
5. 开启“自动跟随”，在另一台电脑移动鼠标或触控板，检查键盘是否真正归位。

软件配对码是局域网消息的校验凭据，**不能代替键盘与接收器的硬件配对**。已有受支持设备通常只需设置，不需要修改源码。

设置页支持 `Ctrl+,` / `⌘,` 打开、`Esc` 返回；“取消”不会保存修改。当前界面没有独立的对端通道入口，非默认通道组合还需确保配置文件中的 `channel` 与 `peerChannel` 在两端互相对应。

Windows 关闭窗口后仍驻留托盘；Mac 默认驻留菜单栏、不占 Dock。通过设置页“退出键来”结束应用，两端均可开启开机自启。

## 工作方式与限制

```text
目标电脑检测本地输入 → 局域网召回请求 → 当前持有方切换键盘 → 目标方确认键盘上线
```

- TCP 是主链路，UDP 广播辅助发现，默认端口均为 `45823`；Windows 会在本地 `/24` 网段探测对端。
- 配对码用于签名校验；消息有时效和重复请求检查，但通信不加密。配置及配对码保存在本机，没有云账号、遥测或公网中继。
- macOS 使用原生输入监听，Windows 使用 Raw Input；不记录按键内容。
- 当前持有方深度休眠时，仍需用 Easy-Switch 手动切换。
- 已验证现有双机的安装启动、单进程、端口、握手，并有实际键盘到达记录；仍存在超时历史，尚未完成长期稳定性与全部兼容性验收。

详见 [技术架构](docs/ARCHITECTURE.md)、[设计规格](docs/design/SPEC.md) 与 [安全说明](SECURITY.md)。

## 排查与反馈

先确认手动切换正常、配对码一致、通道互相对应、键盘已唤醒，以及接收器、网络与权限可用。关闭“自动跟随”即可停止自动召回。

Windows 在源码目录运行只读诊断：

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\diagnose-windows.ps1
```

脚本检查进程、UDP、防火墙、网卡和设备，不修改设置，不输出配对码明文；它不检查 TCP。公开反馈前仍须删除配对码指纹、主机名、IP、设备标识等个人信息。

`scripts/repair-windows-network.ps1` 会请求管理员权限，创建本地子网 UDP 入站规则并重启应用；它不处理 TCP 或出站规则，只应在确认需要时使用，不要关闭整个防火墙。

普通问题请提交 [Issue](https://github.com/s840207702/KeyClaim/issues)，安全问题按 [SECURITY.md](SECURITY.md) 私下报告。

## 开发与许可证

构建依赖、目录职责、检查命令与本地发布方法统一放在 [开发与贡献说明](CONTRIBUTING.md)。GitHub Actions 保持关闭，不会因提交自动构建。

自有代码采用 [MIT License](LICENSE)，允许修改、分发和商用，须保留版权及许可声明，不提供担保。HID++ 协议与部分 Mac 接收器逻辑参考 [mxswitch](https://github.com/marcocosta97/mxswitch)，第三方声明见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。历史开源检查见 [开源准备记录](docs/OPEN_SOURCE_READINESS.md)。
