# NextAI Translator · Windows

用 Rust 和 [GPUI Kit](https://github.com/longbridge/gpui-kit) 重写的 [NextAI Translator](https://github.com/nextai-translator/nextai-translator) 翻译专用版，无需 WebView 或浏览器。

![Windows 实机翻译窗口](docs/translator-dark.png)

## 安装与更新

从 [GitHub Releases](https://github.com/JamesFlare1212/nextai-translator-rust/releases/latest) 下载 Windows x64 版本：`*-portable.exe` 为便携版，`*-setup.exe` 为安装版。便携版请放在有写权限的目录。

应用会后台检查稳定版更新，也可在设置中手动检查。下载后校验大小和 SHA-256，选择「重启并更新」才会安装；旧版若不含更新器，需手动升级。更新前请保存设置和草稿。更新校验不是 Authenticode 签名，Windows 仍可能显示 SmartScreen 提示。

## 使用

1. 打开设置（`Ctrl+,`），填写 API 密钥、选择协议和模型并保存。默认服务为 `https://new-api.jamesflare.com`，默认模型为 `gpt-6-luna`；也支持其他 OpenAI 兼容服务和自定义模型。
2. 输入原文，按 **Enter** 或点击「翻译」；**Shift+Enter** 换行。默认自动识别语言：中文译成英文，其他语言译成设置中的目标语言。顶部可手动选择语言。
3. 译文流式显示，可停止、重试、复制或交换翻译方向；拖动分隔线调整编辑区。右下角可恢复或清空历史。

设置中可调整语言、外观、字体和字号。不同提供商的配置分别保存；思考模型是否可用取决于服务和模型，思考过程不会显示为译文。

### 协议与数据

- 支持 Chat Completions（SSE）、Responses（SSE）和 Responses（WebSocket）。WebSocket 需服务端支持；New API 明确拒绝模型路由时会在本次请求中回退到 Responses SSE。其他不支持 WebSocket 的服务请手动选择 SSE。
- 连接失败或断流时最多自动重试一次，可能再次计费；可点击「停止」取消。输入草稿不会发送，提交、重试或切换翻译方向时才会发送原文。
- 远程服务要求 HTTPS，仅本机回环地址允许 HTTP。应用不内置密钥或模型。
- 设置和最近 100 条成功翻译保存在 `%APPDATA%\NextAI Translator Rust\`；密钥和历史正文由当前 Windows 用户的 DPAPI 保护。
- 本项目仅提供 Windows 桌面翻译，不含浏览器插件、OCR、TTS 或划词监听等功能。

## 构建与测试

在 Windows x64 安装稳定版 Rust（MSVC）、Visual Studio C++ Build Tools 的「使用 C++ 的桌面开发」和 Windows SDK（包含 `fxc.exe`），然后运行：

```powershell
cargo fmt --all -- --check
cargo clippy --locked --release --all-targets --target x86_64-pc-windows-msvc -- -D warnings
cargo test --locked --release --all-targets --target x86_64-pc-windows-msvc
cargo build --locked --release --target x86_64-pc-windows-msvc
.\target\x86_64-pc-windows-msvc\release\nextai-translator.exe
```

[GitHub Actions](.github/workflows/windows.yml) 构建并测试 Windows 程序和安装包。PR 与非默认分支运行离线测试；默认分支推送或手动运行还会使用仓库 Secret `NEW_API_KEY` 进行实网测试。发布时更新 `Cargo.toml` 和 `Cargo.lock` 的版本号，再推送对应的稳定版标签（如 `v0.1.10`）。

本地实网测试仅使用 `https://new-api.jamesflare.com` 和 `gpt-6-luna`。通过 T3 密钥卡获取临时文件，将**文件路径**设为 `NEXTAI_TEST_KEY_FILE`，再运行：

```powershell
cargo test --locked --release new_api_gpt_6_luna -- --ignored
```

实机布局和验证范围见[验收记录](docs/verification.md)。

## 来源

界面、语言选项、操作、简繁体识别字符表和图标参照上游 [`3621e59`](https://github.com/nextai-translator/nextai-translator/tree/3621e591681de518718cc5bc9a1acf18ecd74b16)。本项目是独立定制版本，采用 [AGPL-3.0](LICENSE)。
