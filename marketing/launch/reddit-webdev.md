# Reddit · r/webdev 文案（替代品角度）

## 标题
```
I got tired of a 310 MB API client asking me to sign in, so I built a 10 MB offline-first one (open source)
```

## 正文

Every backend / full-stack dev knows the ritual: install a giant Electron app,
sign in (again), wait for the workspace to sync, and only then send the HTTP
request you actually needed 30 seconds ago.

I built Miyo to delete that ritual. It's a free, open-source (Apache-2.0) API
client for macOS / Windows / Linux:

- **~10 MB installer, <1 s cold start** — it uses your OS's native WebView
  instead of bundling Chromium
- **No account, offline-first** — everything lives in a local SQLite file;
  sensitive variables (tokens, signing secrets) are AES-256-GCM encrypted
  with a key that never leaves your machine
- **Debug**: HTTP, WebSocket, SSE, GraphQL, gRPC; environments with
  `{{variable}}` interpolation; cURL paste-to-import; multi-tab with
  per-tab drafts
- **Built-in mock server** — frontend and backend stop blocking each other;
  rules support delays and fault injection so you can test your error
  handling too
- **Tests that leave the GUI**: write assertions + variable chains (extract a
  token in request A, inject it into request B), then run the whole collection
  headlessly: `miyo-cli run collection.json --junit report.xml` — drop that
  into GitHub Actions and your API regressions fail the pipeline
- **Load testing built in**: concurrency, QPS, P50/P90/P99
- **AI assistant integration**: an MCP server ships with the app — tell
  Claude Code / Cursor "save this cURL to Miyo" and it's done

Migration is easy: it imports OpenAPI 3, Swagger 2, Postman collections and
raw cURL. Your history stays local; backup is a single JSON you can re-import
anywhere.

What it doesn't have (yet): cloud sync / team permissions (local git & S3
sync exist today), and the CLI doesn't do load tests yet.

Repo + downloads: https://github.com/weihubeats/Miyo
Site with full comparison table (size / cold start / memory, measured):
https://weihubeats.github.io/Miyo/

Happy to answer questions — especially from anyone who's evaluated Bruno or
Insomnia and hit a wall.

---

*注意：r/webdev 需要 9:1 的自我推广比例，发帖前确认账号历史。评论区有人问
"Bruno 不就够了吗" 的标准回答：Bruno 是 git-first 的文件哲学，很适合工程
师个人；Miyo 补的是 GUI 优先的团队（测试同学不用碰 git）+ 内置 mock/压测
+ 账号无关的本地加密存储，两者不冲突。*
