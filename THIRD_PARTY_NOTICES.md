# 第三方开源声明

KeyClaim 的 Logitech HID++ `0x1814`（ChangeHost）实现参考了
[mxswitch](https://github.com/marcocosta97/mxswitch) 的协议发现与报文结构。

mxswitch Copyright (c) 2026 Marco Costa，按 MIT License 发布。上游许可证：
https://github.com/marcocosta97/mxswitch/blob/main/LICENSE

macOS IOKit 回调中的 HID 报告规范化逻辑亦由该 MIT 项目改写而来。

以下保留上游 MIT 许可全文；KeyClaim 的 MIT 许可不替代任何第三方权利声明。

```text
MIT License

Copyright (c) 2026 Marco Costa

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## 第三方依赖

项目使用 React、Tauri 及插件、hidapi、Tokio 等依赖，版本以 `package-lock.json` 与 `src-tauri/Cargo.lock` 为准。

- [依赖版本和许可证清单](docs/licenses/DEPENDENCIES.md)
- [已收集的版权声明及许可全文](docs/licenses/THIRD_PARTY_LICENSES.txt)

清单是当前双平台依赖的保守合集，包含构建工具，不代表所有条目都进入最终安装包。第三方依赖保留各自许可证；不能仅用项目的 MIT 许可覆盖它们。

涉及 MPL-2.0 的依赖，其对应版本源码可从清单链接的 crates.io/npm 源码包取得；KeyClaim 没有修改这些依赖源码。再分发者若修改相关文件，应自行履行对应许可证的源码提供义务。其他依赖的版权、许可和 NOTICE 也应随再分发保留。

0.2.6 安装包产生于开源文档补齐之前，包内未核验包含全部第三方声明，因此本次另外发布与安装包一同分发的许可附件。再分发 0.2.6 时请同时保留该附件；后续安装包仍需完善包内许可展示，不能把这份清单当作“已嵌入安装器”的证明。
