# NextAI Translator · Windows

用 Rust 和 [GPUI Kit](https://github.com/longbridge/gpui-kit) 重写的 [NextAI Translator](https://github.com/nextai-translator/nextai-translator) 翻译专用版。窗口使用 Windows 原生毛玻璃，保留原版的语言栏、上下对照编辑区、流式结果和底部工具栏；不依赖 WebView 或浏览器。

![Windows 实机翻译窗口](docs/translator-dark.png)

## 开始翻译

1. 运行 `nextai-translator.exe`。点左下角齿轮（`Ctrl+,`），选择服务、填写模型和密钥，点「保存」。使用本机 Ollama 时，先启动 Ollama 并下载模型，例如 `ollama pull mistral`；选择 Ollama 预设，密钥留空。
2. 输入或粘贴原文，按 **Enter** 或点「翻译」；**Shift+Enter** 换行。默认识别原文语言：中文译成英文，其他语言优先译成设置中的目标语言。也可以直接在顶部搜索、指定语言。单独提交一个词时会请求词典式译文。
3. 译文逐步显示。「停止」中断请求；结果下方可以重试或复制。交换箭头会把已完成的译文放回输入框并反向翻译。拖动中间的分隔线可调整上下区域。
4. 右下角可恢复或清空翻译历史；恢复记录不重新请求模型。设置支持跟随系统、浅色和深色外观。系统关闭透明效果时，毛玻璃表现以 Windows 合成器为准。

### 模型与本地数据

- 网络接口使用 **OpenAI Chat Completions**。支持 OpenAI、Ollama 等预设和自定义完整端点，例如 `http://127.0.0.1:11434/v1/chat/completions`。模型名必须在对应服务可用；不内置模型、账号或免费额度。不使用原版的浏览器会话登录。
- 输入草稿不会发送请求。提交、重试或在有原文时切换翻译方向，才会把原文发送给所选服务。远程地址要求 HTTPS；仅本机回环地址允许 HTTP。密钥放在专门的字段，不放在 URL 中。
- 设置和最近 100 条成功翻译位于 `%APPDATA%\NextAI Translator Rust\`。密钥和历史正文使用当前 Windows 用户的 DPAPI 保护。清空历史不会删除设置。
- 只提供 Windows 桌面应用；没有浏览器插件、润色、总结、TTS、截图/OCR、生词本或记忆内容生成，也不监听其他应用的划词或剪贴板。

## 构建与测试

在 Windows x64 安装稳定版 Rust（MSVC）、Visual Studio C++ Build Tools 的「使用 C++ 的桌面开发」和 Windows SDK（包含 `fxc.exe`）。在项目目录运行：

```powershell
cargo fmt --all -- --check
cargo clippy --locked --release --all-targets --target x86_64-pc-windows-msvc -- -D warnings
cargo test --locked --release --all-targets --target x86_64-pc-windows-msvc
cargo build --locked --release --target x86_64-pc-windows-msvc
.\target\x86_64-pc-windows-msvc\release\nextai-translator.exe
```

[GitHub Actions](.github/workflows/windows.yml) 在推送和拉取请求时执行上述检查、构建 Windows x64 程序，并上传 `nextai-translator-windows-x64`。测试不使用外部 API 或真实密钥。实机布局和真实模型的检查结果见[验收记录](docs/verification.md)。

## 来源

界面、语言选项与操作参照上游 [`3621e59`](https://github.com/nextai-translator/nextai-translator/tree/3621e591681de518718cc5bc9a1acf18ecd74b16) 的 `Translator.tsx`、`lang/data.ts`、`translate.ts` 和桌面窗口定义。简繁体识别字符表来自同一版本的 `traditional-or-simplified.ts`，应用图标来自 `src-tauri/icons/icon.ico`。本项目是独立定制版本，采用 [AGPL-3.0](LICENSE)。
