# Reddit · r/devops 文案（CI 角度）

## 标题
```
API regression gates in CI: write tests in a GUI, run them with a 5 MB Rust CLI (JUnit out, open source)
```

## 正文

Common pattern on teams I've worked with: someone writes great API assertion
tests in Postman, they live in a workspace, and running them in CI means
exporting to Newman and maintaining a parallel copy that drifts. Or tests
don't exist at all because "we'll add automation later."

I built an open-source (Apache-2.0) tool where the GUI and the CI runner are
the same engine — **Miyo**:

- Write tests in the desktop app: per-request pre-request variables, value
  extraction (grab a token from the login response), assertions (status,
  jsonpath, headers, body, response time), chained into flows
- Export the project as a single JSON (built-in backup function)
- Run it headlessly: `miyo-cli run collection.json --env production --junit report.xml`
  - exit code 0 = all passed, 1 = regression, 2 = bad input
  - JUnit XML for GitHub Actions / GitLab
  - ~5 MB static Rust binary, no runtime, no browser
- Same engine, so "passed locally, failed in CI" can't happen due to
  semantics drift (it can still happen because prod is on fire — the CLI
  won't fix that)

The CLI also supports `--endpoint <filter>` for targeted runs, `--fail-fast`,
`--var k=v` injection (highest priority over environment variables baked into
the collection), and per-request timeouts.

Under the hood it's the same Rust test engine the GUI uses — the CLI is a
thin clap wrapper, which is exactly why it stays in sync.

Repo (with docs + a GitHub Actions example):
https://github.com/weihubeats/Miyo

The GUI itself is a ~10 MB offline-first API client (mock server, WS/SSE/
GraphQL/gRPC debugging, load testing) if you also want a daily driver — but
the CLI works standalone.

Questions about the approach welcome. If your API tests already live in
Postman, the import path is Postman → Miyo (collections + environments).

---

*注意：r/devops 关心的是可靠性与流水线集成，正文直接给命令和退出码语义。
若被问 "为什么不用 newman"：newman 需要 Node 运行时且集合导出格式与 GUI
有漂移历史；这里 GUI 与 CLI 共享同一 Rust 引擎。若被问 "敏感数据"：集合
里敏感变量本地 AES-256-GCM 加密；CI 里用 --var 注入 secrets，不落盘。*
