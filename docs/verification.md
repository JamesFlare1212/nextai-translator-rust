# Windows 版验收记录

验收对象是 **Windows x64、仅翻译的原生桌面版**。比对基线是 NextAI Translator [`3621e59`](https://github.com/nextai-translator/nextai-translator/tree/3621e591681de518718cc5bc9a1acf18ecd74b16)，不是原项目的浏览器弹窗。未把外部账号、上游的所有专有服务协议或额外工具当作翻译的前提。

| 要求 | 实现与验证 |
| --- | --- |
| Rust、Longbridge GPUI Kit、仅 Windows | `Cargo.toml` 固定 `gpui-kit = 0.11.0`；`src/main.rs` 创建原生窗口。Cargo 元数据只有一个应用二进制和一个资源构建脚本。本机 release 的 PE 头为 `AMD64 / WINDOWS_GUI`，已在 Windows 启动并关闭，无 WebView 运行时。 |
| 复刻翻译界面与操作 | 参照 `src-tauri/src/windows.rs` 的 620×700 / 最小 540×600 逻辑像素，以及 `src/common/components/Translator.tsx`。在浏览器中运行上游桌面渲染器取得对照，并在 Windows 原生窗口以 150% 缩放检查了顶部语言栏、可调输入区、译文与状态分隔线、底栏。已移除的模式按钮不占位。见[默认尺寸](translator-dark.png)、[最小尺寸的语言菜单](languages-minimum.png)。 |
| 设置与焦点 | 150% 缩放实机检查：模型行使用单个全宽选择框，不另设模型输入框或刷新按钮。540 px 宽的 GPUI 交互测试覆盖打开时拉取列表、搜索并选中服务模型，以及使用自定义名称。默认字体 MiSans，验证了更换 Windows 已安装字体、15→18 px 后保存生效。原文框在有、无输入焦点时均为同一细边框。浅色、深色下实机展开字体和字号菜单：触发框没有外圈，仅原有细边框对比度略增。 |
| 毛玻璃 | `WindowBackgroundAppearance::Blurred` 使用 Windows 合成器，GPUI 根节点透明，编辑区、顶栏、底栏用可透出背景的表面。在同一桌面检查了浅色和深色窗口，不是预制背景图。 |
| 翻译确实工作 | Windows `.exe` 通过 `https://new-api.jamesflare.com` 使用 `gpt-6-luna`。Responses 开启思考时，**The train will arrive in ten minutes.** 译为 **火车将在十分钟后到达。**，没有把推理事件显示为译文。 |
| 等待、失败与恢复 | 另用本地 HTTP 测试服务生成可重复的状态：流式文本出现后 Esc 停止，服务端观察到断连；返回 HTTP 429 后点重试，服务端收到第二次请求并返回结果；在最小窗口滚动到长译文第 70 段并清空。没有以模拟翻译替代真实模型测试。 |
| 无浏览器插件和桌面划词扩展 | 只有桌面应用入口，没有扩展清单、内容脚本、全局选区监听、鼠标钩子或桌面截取路径；`src/ui.rs` 的复制操作限于窗口内。 |
| 无润色与总结 | `src/engine.rs::translate` 只有翻译与单词释义请求；工具栏和设置没有其他文本任务。语言列表中的 `Polish` 是波兰语。 |
| 无 TTS 与截图翻译 | 没有朗读、语音合成、截图、OCR、图片上传处理或对应设置。GPUI Kit 启用 `component`、`assets`，不启用语音功能。 |
| 无生词本和记忆内容生成 | `src/history.rs` 保存完整原文、译文及方向，用于恢复翻译，不收集单词或生成记忆文章。UI 仅有翻译、设置和历史三个视图。 |
| GitHub CI | `.github/workflows/windows.yml` 配置 push、PR 和手动触发；Windows 执行格式、Clippy、测试、MSVC release 构建并上传可执行文件。工作流通过 `actionlint`。仅默认分支 push 和该分支的手动运行额外执行实网测试。仓库 Secret `NEW_API_KEY` 只注入该步骤；PR 不执行实网测试。 |

本地运行 `cargo fmt --all -- --check`、`cargo clippy --release --locked --all-targets --target x86_64-pc-windows-gnu -- -D warnings`、`cargo test --release --locked --all-targets --target x86_64-pc-windows-gnu` 和相同目标的 `cargo build --release --locked` 均通过。27 项离线测试通过（含切换浅色、深色和跟随系统后的焦点样式）；另行显式运行了 `live_tests::new_api_gpt_6_luna`。它先检查真实模型列表，再分别以 Chat Completions 和 Responses 测试思考开、关四种组合。离线测试覆盖两种流式协议、凭据保护、旧设置迁移、模型列表与自定义输入、历史、语言方向，以及 GPUI 组件上的输入、换行和复制等操作。Windows GNU 是本机验证目标；仓库 CI 配置使用 Windows MSVC，两者没有混称为同一次运行。
