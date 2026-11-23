# 测试约定

- 常规测试与 GitHub CI 使用无密钥的单元、GPUI 和本地模拟 HTTP 测试。
- 凡需真实模型的端到端验证，只使用 `https://new-api.jamesflare.com` 和 `gpt-6-luna`；不要下载或启动本地 Ollama、llama.cpp、Qwen 模型。
- 通过 T3 的 `request_secret` 获取密钥文件，不将密钥放在命令行、仓库、日志或截图里。见 README 的实网测试命令。
