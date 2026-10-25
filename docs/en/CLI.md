# CLI Test Runner (miyo-cli)

Run API test collections headlessly: hand a project backup JSON exported from the desktop app to `miyo-cli`, execute every endpoint with embedded tests in your terminal / CI, and get a human-readable summary plus a JUnit XML report. The runner is the exact same engine the desktop app uses (`miyo-test`), so pre-request scripts, variable extraction and assertion semantics are identical.

## Build & install

```bash
cargo build --release -p miyo-cli
# artifact: target/release/miyo-cli
```

## The collection file

The input is the **project backup JSON** exported from the desktop app (Settings → Backup & Restore) — zero new formats. Endpoint parameters, auth and the embedded tests you wrote in the debug view's **Tests** tab (pre_request / extract / assertions) all take effect as-is.

## Usage

```bash
miyo-cli run ./miyo-backup.json \
  --env production \
  --var token=abc123 \
  --junit report.xml
```

| Argument | Description |
| --- | --- |
| `backup` (positional) | Project backup JSON exported from the desktop app |
| `--env <name>` | Pick an environment: takes its Base URL and variables; optional when the backup has exactly one environment |
| `--base-url <url>` | Override the Base URL (takes priority over `--env`; supports `{{variables}}`) |
| `--endpoint <substr>` | Only run endpoints whose name contains this substring (case-insensitive, repeatable) |
| `--var <k=v>` | Inject variables (repeatable, highest priority) |
| `--timeout-ms <n>` | Per-request timeout in milliseconds |
| `--junit <path>` | Write a JUnit XML report (GitHub Actions / GitLab compatible) |
| `--fail-fast` | Stop on the first failure |

## Output & exit codes

```text
▶ Project: Acme · Users
  ✓ GET /users — 200 · 109ms · assertions 3/3
  ✗ POST /users — 500
      ✗ status eq 200: expected 200, got 500
  ⊘ GET /grpc-stream — skipped: gRPC debugging is not supported in the CLI
Summary: 8 passed / 1 failed / 1 skipped (12 endpoints, 3.4s)
```

- Exit code `0`: all passed; `1`: at least one failure; `2`: bad input (missing file / parse error).
- Endpoints without embedded tests are not executed or counted (same semantics as the desktop app's "no test config → skip").

## GitHub Actions example

```yaml
- name: Run Miyo API collection
  run: |
    miyo-cli run ./collection.json --env production --junit report.xml
- uses: actions/upload-artifact@v4
  if: always()
  with:
    name: junit
    path: report.xml
```

## Current boundaries

- gRPC endpoints are skipped (debug them in the desktop editor);
- Load testing is not exposed via the CLI yet;
- The variable table is in-memory per run — `{{$seq}}` counters start at 1 (the desktop app's persisted sequence counters are not consulted).
