# Reddit · r/rust 文案（技术角度）

## 标题
```
I built an API client in Tauri 2 + Rust: 10 MB installer, 17-crate domain layer, 459 tests — here's the architecture
```

## 正文

I spent ~8 months of evenings building Miyo, a cross-platform API client
(HTTP/WS/SSE/GraphQL/gRPC debugging + mock server + test runner + load
testing). It's Apache-2.0: https://github.com/weihubeats/Miyo

This post is about the Rust architecture, not the product pitch.

**The shape**: a Tauri 2 plugin exposes ~40 IPC commands; below it sit 17
domain crates (miyo-http, miyo-storage, miyo-test, miyo-grpc, miyo-mock,
miyo-agent, ...) with zero UI dependencies. The CLI (`miyo-cli`) reuses the
same test engine the GUI uses — one binary runs your API collections in CI
and emits JUnit.

**Things that worked well**:

- `OnceLock<RwLock<ClientPair>>` for shared reqwest clients with per-request
  timeout overrides — connection pooling survives across tabs
- SQLite via sqlx with a migration safety net: WAL checkpoint + full-file
  snapshot into `snapshots/` (keep 5) before every migration, then
  `PRAGMA integrity_check` after. Failed migrations are roll-backable
- Sensitive values (env variables, signing secrets) encrypted with
  AES-256-GCM before they ever touch the DB; master key lives in the data
  dir with 0600, so "copy the directory = complete backup" still holds
- Contract tests for our own IPC surface: the `generate_handler!` command
  list, the Tauri permission allowlist and the build.rs COMMANDS table are
  cross-checked in both directions. Silent permission rejections can't ship

**Things that bit us**:

- sqlx migration files are checksum-immutable. A mass-rename refactor touched
  comments inside already-applied migrations → every existing DB refused to
  boot. Rule now: migrations are append-only, enforced by review checklist
- Tauri embeds window icons at compile time via a proc-macro — replacing the
  icon file doesn't rebuild the dev binary until you touch build inputs
- Async-shell sync-IO creep: full-file copies and snapshot writes quietly
  migrate into async fns and stall runtime workers. We keep a recurring
  audit: grep `std::fs` inside async fns
- Streaming buffers need hard caps everywhere a user controls volume (gRPC
  stream messages: 500, response bodies: 20 MB, log tails: 512 KB)

**Numbers**: ~10 MB installer (system WebView + LTO + tokio/axum feature
trimming), <1s cold start, ~40 MB RSS, 459 tests, 4-platform CI.

Repo (architecture diagram, tests, docs in EN/中文):
https://github.com/weihubeats/Miyo

Happy to go deeper on the Tauri permission-contract approach or the SQLite
crypto design if there's interest.

---

*注意：r/rust 反感营销。只讲工程，产品链接放文末一次，评论区如实回答。若被问 "why Tauri not egui/iced"：需要富文本 + 可访问性 + 三端 WebView 一致性，WKWebView/WebView2 成熟度是决定因素。*
