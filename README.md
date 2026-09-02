# KeyClaim

让 MX Keys 跟随本地鼠标或触控板，在 Windows 通道 2 与 Mac 通道 3 之间自动归位。

## 工作方式

KeyClaim 需要在两台电脑上同时运行。目标电脑检测到本地指针活动后，通过局域网向当前持有键盘的电脑发送签名召回请求；持有方确认目标在线、就绪，再用 Logitech HID++ `ChangeHost` 切换键盘。

- 仅在实际输入后召回，不因系统单纯唤醒而切换。
- 目标电脑离线或未就绪时不切换。
- 没有 2 秒锁定，默认只有 60ms 输入防抖。
- macOS 的触控板和 Master 3S 都能触发。
- 所有状态与配置只保存在本机。

更完整的流程见 [技术架构](docs/ARCHITECTURE.md)，界面标准见 [设计规格](docs/design/SPEC.md)。

## 开发

```bash
npm install
npm run tauri dev
```

常用检查：

```bash
npm test
npm run build
cd src-tauri && cargo test
```

## 当前设备配置

- Windows 工作站：MX Keys 通道 2，MX Master 3。
- MacBook Pro：MX Keys 通道 3，MX Master 3S 与内置触控板。
- macOS 优先寻找 PID `0xC52B` 的 Unifying 接收器，避免误控制 PID `0xC548` 的 Bolt 鼠标。

## 已知边界

第一版输入监听使用全局光标位移，远程控制/模拟输入过滤仍是实验性能力。正式长期运行前，需在 macOS 和 Windows 分别完成原生事件来源过滤，并在真实双机上验收睡眠、唤醒、离线和往返切换。

## 开源声明

HID++ 协议实现参考 `mxswitch`（MIT），详见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
