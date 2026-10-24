# CLI 测试运行器（miyo-cli）

无头运行 API 测试集合：把桌面端导出的项目备份 JSON 交给 `miyo-cli`，在终端 / CI 里执行其中带内嵌测试的接口，输出人读结果与 JUnit XML 报告。运行器与桌面端共享同一套引擎（`miyo-test`），断言 / 变量提取 / 变量链语义完全一致。

## 构建与安装

```bash
cargo build --release -p miyo-cli
# 产物：target/release/miyo-cli
```

## 集合文件

即桌面端「设置 → 备份与恢复」导出的**项目备份 JSON**——零额外格式，界面里已写好的接口参数、认证、内嵌测试（调试页「测试」页签的 pre_request / extract / 断言）全部原样生效。

## 用法

```bash
miyo-cli run ./miyo-backup.json \
  --env 开发环境 \
  --var token=abc123 \
  --junit report.xml
```

| 参数 | 说明 |
| --- | --- |
| `backup`（位置参数） | 桌面端导出的项目备份 JSON |
| `--env <名称>` | 选择环境：取其 Base URL 与变量；备份只有一个环境时可省略 |
| `--base-url <url>` | 覆盖 Base URL（优先级高于 `--env`；支持 `{{变量}}`） |
| `--endpoint <子串>` | 只运行名称匹配的接口（大小写不敏感，可多次取并集） |
| `--var <k=v>` | 注入变量（可多次，优先级最高） |
| `--timeout-ms <n>` | 单请求超时（毫秒） |
| `--junit <path>` | 输出 JUnit XML（GitHub Actions / GitLab 通用格式） |
| `--fail-fast` | 任一失败立即停止 |

## 输出与退出码

```text
▶ 项目：小奏技术 · 用户服务
  ✓ GET /users — 200 · 109ms · 断言 3/3
  ✗ POST /users — 500
      ✗ status eq 200：期望 200 实际 500
  ⊘ GET /grpc-stream — 跳过：CLI 暂不支持 gRPC 调试
汇总：8 通过 / 1 失败 / 1 跳过（12 个接口，耗时 3.4s）
```

- 退出码 `0`：全部通过；`1`：存在失败；`2`：输入错误（文件不存在 / 解析失败）。
- 未写内嵌测试的接口不运行也不计入（与桌面端「无测试配置跳过」口径一致）。

## GitHub Actions 示例

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

## 当前边界

- gRPC 接口跳过（桌面端编辑器内调试）；
- 压测（load）暂未暴露到 CLI；
- 变量表为运行时内存态，`{{$seq}}` 自增序列从 1 起（桌面端持久化的序列计数不参与）。
