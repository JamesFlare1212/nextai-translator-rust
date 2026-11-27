# Windows x64 验收标准

桌面布局以 NextAI Translator [上游版本 3621e59](https://github.com/nextai-translator/nextai-translator/tree/3621e591681de518718cc5bc9a1acf18ecd74b16) 和[默认尺寸基线图](translator-dark.png)为参照。

## 原生窗口

- 在 Windows 150% 缩放下，分别以默认 620×700、最小 540×600 逻辑像素检查浅色和深色主题。语言栏、原文框、译文、状态栏和底栏不应重叠或被裁切。
- 展开语言和设置菜单：选项圆角 6 px、四周内距 4 px、面板圆角 10 px，阴影不计入内距。思考开关的轨道和滑块圆角分别为 10 px 和 8 px，端点内距为 2 px。
- 原文框获得或失去焦点时都保留单层细边框；字体和字号菜单展开时不增加外圈。在最小窗口打开设置，用同一个模型选择框搜索、选中服务模型或输入自定义名称；保存后重新打开，核对模型、字体和字号。

## 翻译与更新

- [Chat Completions](https://developers.openai.com/api/reference/resources/chat/subresources/completions/methods/create) 和 [Responses](https://developers.openai.com/api/reference/resources/responses/streaming-events) 的 HTTP 翻译请求须设置 `stream: true`，只接受所选协议的 `text/event-stream`。普通 JSON、错误媒体类型或无法解码的事件应报错，不自动重发；`GET /models` 仍使用 JSON。
- 单独验证 [Responses WebSocket](https://developers.openai.com/api/docs/guides/websocket-mode)：握手成功不等于翻译完成。各传输仅显示可见译文，截断、拒绝或不完整输出不算成功；检查取消和读入限长，重试不得切换协议。
- 在 Windows 临时目录用便携版 EXE 副本验收更新：校验失败不替换，新程序无法启动时恢复旧版，成功后从原路径启动新版。安装版另验 Inno Setup 安装与卸载。

记录验收时注明版本、窗口尺寸和缩放，区分真机界面检查、本地模拟协议测试与实网翻译。实网只用 `https://new-api.jamesflare.com` 的 `gpt-6-luna`，按实际测试的协议和思考开关分别记录结果，不以模拟结果代替实网结论。
