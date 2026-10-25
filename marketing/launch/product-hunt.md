# Product Hunt 文案

## Name
Miyo

## Tagline（≤60 字符）
```
A 10 MB offline-first API client with mock, testing & CLI
```

## Description（一句话扩展）
```
Miyo is a lightweight, open-source API client (Rust + Tauri 2): debug HTTP,
WebSocket, SSE, GraphQL and gRPC, spin up a built-in mock server, run
assertion tests, and gate your CI with the bundled CLI — all offline, no
account, in a ~10 MB installer.
```

## Gallery 图（5 张，顺序即叙事）
1. 首页仪表盘（浅色主题）——第一印象："这么轻？"
2. 工作台发送请求 + JSON 响应——核心调试场景
3. Mock Server 配置页——"不用等后端"
4. 断言测试 + 变量链——"测试写在 GUI 里"
5. 终端跑 miyo-cli 输出 JUnit——"还能进 CI"（最有记忆点的一张放最后）

## Maker Comment（发布后置顶）

Hi PH! 👋 I'm a backend developer who got tired of a 310 MB API client that
wanted me to sign in before debugging my own company's internal endpoints.

So I built Miyo over ~8 months of evenings, in Rust + Tauri 2:

🔎 **Debug** HTTP, WebSocket, SSE, GraphQL, gRPC — with environments, {{variable}}
   interpolation and AES-256-GCM-encrypted secrets
🧪 **Test** your APIs with assertions and variable chains, right in the GUI
🤖 **Mock** unfinished dependencies with a built-in server (delays & fault injection)
⚡ **Gate your CI** — the same test engine runs headlessly via miyo-cli with JUnit output
📊 **Load test** with QPS and P50–P99 percentiles, built in
🤝 **AI-ready** — built-in MCP server: save a cURL to Miyo by asking your AI assistant

No account. Everything local (SQLite + encrypted secrets). Apache-2.0.
~10 MB installer for macOS, Windows and Linux with auto-update.

I'd love your feedback — especially from teams that tried to move off Postman.
What's the one thing that would make this a daily driver for you?

🔗 https://github.com/weihubeats/Miyo · https://weihubeats.github.io/Miyo/

## 发布注意
- 周二~周四 00:01 PT 发布（吃满全天）
- 提前找 5-10 个真实用户约定发布日上线支持（真诚使用反馈，不刷票）
- 每条评论 24 小时内回复
- Gallery 图统一加浅色背景 + 圆角，风格一致
