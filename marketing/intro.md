# Miyo：一个 10MB 的 API 调试工具，把 Mock、测试、压测和 CI 全做进去了

> 产品介绍文，主投掘金 / 少数派 / 即刻，配动图使用。文末有下载与反馈入口。

做后端的同学每天都在和接口打交道：调接口、Mock 未完成的依赖、给核心链路写断言测试、上线前压一波。这四件事你现在的工具链可能是：Postman（调试）+ 某个 Mock 平台 + Postman Tests + JMeter，外加一个 300MB+ 的 Electron 应用和一次强制登录。

**Miyo 把这四件事装进了一个 10MB 的本地应用里**，无账号、离线可用、Apache-2.0 开源。这篇文章带你 5 分钟过一遍它到底能干什么。

## 一、调试：该有的都有，不该卡的都不卡

HTTP 全方法、WebSocket / SSE / GraphQL / gRPC、环境变量（支持 `{{变量}}` 插值 + AES-256-GCM 加密）、Cookie 自动回放、全局代理、cURL 一键导入、多项目多标签（草稿独立，切页签不串响应）。

请求执行引擎是 Rust 的 reqwest：冷启动 < 1 秒，常驻内存 ~40MB。**调试工具自己不应该成为你机器上的负担**——这是 Miyo 的第一原则。

## 二、Mock Server：前后端不用再互相等

内置一个 axum Mock 服务，端口从 4010 起自动探测：

- 按路径 / 方法 / Header 匹配规则，优先级可调；
- 响应支持模板变量，可配延迟和故障注入（超时、500）——联调异常分支不用求后端改代码；
- 一键启停，热加载。

## 三、断言测试：写在 GUI 里，跑在 CI 里

接口的"测试"页签里写前置变量、提取和断言（status / jsonpath / header / body / 响应时间），可以串成变量链：接口 A 提取的 token 自动注入接口 B。

关键在下面这一步——同一个集合交给 `miyo-cli` 无头运行：

```bash
miyo-cli run ./collection.json --env production --junit report.xml
```

```text
▶ 项目：小奏技术 · 用户服务
  ✓ GET /users — 200 · 109ms · 断言 3/3
  ✗ POST /users — 500
      ✗ status eq 200：期望 200 实际 500
汇总：8 通过 / 1 失败 / 1 跳过（12 个接口，耗时 3.4s）
```

JUnit 报告丢进 GitHub Actions / GitLab CI，**接口回归，流水线直接红**。GUI 调试和 CI 门禁跑的是同一套 Rust 引擎，不存在"本地过 CI 挂"的口径差。

## 四、压测：顺手的事

并发压测内置：QPS、吞吐、P50/P90/P99 分位耗时、错误样本，结果入库可回看。不用为一个简单的基准测试去学 JMeter。

## 五、其他值得知道的

- **AI Agent 集成**：内置 MCP Server，在 Claude Code / Cursor 里说一句"把这个 cURL 存到用户服务项目"，接口就进来了；
- **团队同步**：项目快照推到 git / S3 / 数据库远端，敏感值推送前自动脱敏；
- **导入**：OpenAPI 3 / Swagger 2 / Postman v2.1 / cURL；
- **备份**：一键导出 JSON，恢复时 UUID 全量重映射，绝不覆盖现有数据；
- **这个备份 JSON 同时就是 CLI 的测试集合**——一套数据三个用途（备份 / 恢复 / CI）。

## 对比一下

| 维度 | Miyo | Postman | Bruno |
| --- | --- | --- | --- |
| 安装包 | ~10 MB | ~310 MB | ~433 MB |
| 账号 | 不需要 | 引导登录 | 不需要 |
| Mock | 内置 | 云端限额 | 无内置 |
| CI 测试 | 内置 miyo-cli | 外挂 Newman | bruno-cli |
| 压测 | 内置 | 付费档 | 无 |
| 协议 | Apache-2.0 | 专有 | MIT |

完整的维度表（内存、冷启动、应用壳）在[官网](https://weihubeats.github.io/Miyo/)，数据可自行验证。

## 下载与反馈

- macOS（Apple Silicon / Intel）、Windows（exe / msi）、Linux（deb / AppImage / rpm）：**[GitHub Releases](https://github.com/weihubeats/Miyo/releases)**
- 应用内自动更新已就绪，装完不用再手动追版本
- 中英文档：[官网 Docs](https://weihubeats.github.io/Miyo/)
- 遇到问题或想要某个功能：[GitHub Issues](https://github.com/weihubeats/Miyo/issues)，每条都会回

Miyo 是 Apache-2.0 开源项目，如果你觉得有用，一个 star 就是最大的鼓励。
