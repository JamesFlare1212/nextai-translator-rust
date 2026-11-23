# NextAI Translator · Windows

用 Rust 和 [GPUI Kit](https://github.com/longbridge/gpui-kit) 重写的 [NextAI Translator](https://github.com/nextai-translator/nextai-translator) 翻译专用版。窗口使用 Windows 原生毛玻璃，保留原版的语言栏、上下对照编辑区、流式结果和底部工具栏；不依赖 WebView 或浏览器。

![Windows 实机翻译窗口](docs/translator-dark.png)

## 开始翻译

1. 运行 `nextai-translator.exe`。点左下角齿轮（`Ctrl+,`），默认服务是 `https://new-api.jamesflare.com`，模型为 `gpt-6-luna`。填写密钥，选择 API 协议；可以拉取模型列表后选择，也可以直接输入模型名。点「保存」。
2. 输入或粘贴原文，按 **Enter** 或点「翻译」；**Shift+Enter** 换行。默认识别原文语言：中文译成英文，其他语言优先译成设置中的目标语言。也可以直接在顶部搜索、指定语言。单独提交一个词时会请求词典式译文。
3. 译文逐步显示。「停止」中断请求；结果下方可以重试或复制。交换箭头会把已完成的译文放回输入框并反向翻译。拖动中间的分隔线可调整上下区域。
4. 右下角可恢复或清空翻译历史；恢复记录不重新请求模型。设置支持跟随系统、浅色和深色外观。字体默认使用 [MiSans](https://hyperos.mi.com/font/zh/download/)，未安装时回退到 Windows 系统字体。设置中可搜索已安装字体，并调整原文和译文的字号（10–36 px）。输入框获得焦点时保留光标，不高亮边框。

### 模型与本地数据

- 支持 **Chat Completions** 和 **Responses API**，都可流式显示译文。API 地址可填写服务根地址、版本路径或完整端点；例如 `https://new-api.jamesflare.com`。模型列表是按需请求，不获取列表也能手动填写并保存。
- 「思考模型」用于请求模型的推理能力，是否支持取决于服务及模型。思考过程不作为译文展示。应用不内置密钥、远程模型配额或本地模型。
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

[GitHub Actions](.github/workflows/windows.yml) 在推送和拉取请求时执行上述检查、构建 Windows x64 程序，并上传 `nextai-translator-windows-x64`。常规测试全部离线，不消耗 API 额度。

实网验收仅使用 `https://new-api.jamesflare.com` 和 `gpt-6-luna`。通过 T3 密钥卡获取临时文件，把**文件路径**设置为 `NEXTAI_TEST_KEY_FILE`，然后运行以下命令。它会检查模型列表，并分别测试两种协议的思考开/关，不输出密钥。

```powershell
cargo test --locked --release new_api_gpt_6_luna -- --ignored
```

实机布局和验证范围见[验收记录](docs/verification.md)。

## 来源

界面、语言选项与操作参照上游 [`3621e59`](https://github.com/nextai-translator/nextai-translator/tree/3621e591681de518718cc5bc9a1acf18ecd74b16) 的 `Translator.tsx`、`lang/data.ts`、`translate.ts` 和桌面窗口定义。简繁体识别字符表来自同一版本的 `traditional-or-simplified.ts`，应用图标来自 `src-tauri/icons/icon.ico`。本项目是独立定制版本，采用 [AGPL-3.0](LICENSE)。
