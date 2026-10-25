# Show HN 文案

## 标题（二选一）

```
Show HN: Miyo – A 10 MB offline-first API client (Rust + Tauri 2)
```

```
Show HN: Miyo – API debugging, mock server, testing and CLI in a 10 MB app
```

（建议第一条，具体数字比功能罗列更容易被点开；HN 标题不要堆关键词。）

## 首评（发布后立刻以作者身份跟帖）

Hi, I'm the author. Quick background:

Like most backend devs I lived in Postman. What finally pushed me to build
something was a combination of: a 310 MB installer for sending HTTP requests,
being asked to sign in to debug my own company's *internal* endpoints, and
realizing my API tests could only ever run inside a GUI.

Miyo is what I ended up with after ~8 months of evenings:

- ~10 MB installer, <1s cold start, ~40 MB resident memory (system WebView,
  Rust backend — numbers you can verify, comparison table on the site)
- No account. Data lives in a local SQLite file; sensitive variables are
  AES-256-GCM encrypted with a local master key
- HTTP / WebSocket / SSE / GraphQL / gRPC debugging
- Built-in mock server (port probing, rules, delay/fault injection)
- Assertion tests with variable chains — and the same engine runs headlessly
  via `miyo-cli run collection.json --junit report.xml`, so the tests you write
  in the GUI become a CI quality gate (JUnit out, exit codes)
- Built-in load testing (QPS, P50–P99)
- MCP server built in: point Claude Code / Cursor at it and save a cURL as an
  endpoint by saying so in one sentence
- Apache-2.0, Windows / macOS / Linux, auto-update

Honest limitations: no cloud sync or team permissions yet (local git/S3 sync
exists), gRPC streaming UI is editor-only, the CLI doesn't do load tests yet.

Links: https://github.com/weihubeats/Miyo (source, architecture diagram,
459 tests) · https://weihubeats.github.io/Miyo/ (site, EN/中文 docs) ·
releases have signed installers + auto-update.

Happy to answer anything about the Tauri 2 architecture, the SQLite+crypto
design, or why we wrote contract tests for our own IPC permissions.

## 发帖注意事项

- 时间：美东周二~周四上午 8:30–10:30
- 标题不带营销词（不要 "amazing"/"powerful"），数字说话
- 首评里主动列 limitations，HN 对诚实非常买账
- 准备好回答 "why not Bruno/Insomnia"：不贬低，讲场景差异（GUI 团队协作 +
  内置 mock/压测 + 本地优先的组合）
- 若被问 "10MB 怎么做到的"：引技术拆解文的要点（系统 WebView + 特性裁剪 +
  LTO），有完整文章支撑
