# 参与 KeyClaim

欢迎提交问题、设备兼容性反馈和小范围改进。请先阅读 [README](README.md) 的适用范围与已知限制。项目目前主要面向 Windows + macOS、MX Keys 与 Logitech 接收器，不承诺所有蓝牙键盘或鼠标均能使用。

## 报告问题

请通过 [GitHub Issues](https://github.com/s840207702/KeyClaim/issues) 提供：

- KeyClaim 版本、操作系统版本与 CPU 架构。
- 键盘、鼠标和接收器型号，以及键盘通过接收器还是蓝牙连接。
- 复现步骤、预期结果、实际结果，以及问题发生的时间。
- 经脱敏的日志片段。Windows 可运行 README 中的只读诊断脚本。

不要上传整个配置文件、配对码或未经检查的诊断 JSON。诊断中的配对码指纹、主机名、IP、设备序列号和完整本机路径也应删除或替换为占位符。安全问题请按 [SECURITY.md](SECURITY.md) 私下报告。

## 提交改动

1. Fork 仓库，创建自己的工作分支。
2. 每次只解决一个问题；不要附带格式化整个项目或无关重构。
3. 配置、真实日志、密钥、签名证书和安装包不要提交到 Git。
4. 在 Pull Request 中说明原因、影响范围、检查命令和结果。涉及设备切换时，写明哪些硬件和双机场景实际测过，哪些没有。

在对应平台安装 README 列出的构建依赖后运行：

```bash
npm ci
npm test
npm run build
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo test --manifest-path src-tauri/Cargo.toml --locked
```

单元测试通过不等于真实设备切换通过。需要双机验收的修改，请检查安装后的唯一进程、UDP/TCP 端口归属、输入监听、网络握手和键盘真正到达目标通道。

项目维护者当前选择本地构建与手动发布；GitHub Actions 保持关闭，不要求贡献者启用付费服务。

提交贡献表示你有权提供该改动，并同意按本项目的 MIT 许可证发布。引用或改写第三方代码时，保留对应版权声明与许可证。
