# 把一个全功能桌面应用塞进 10MB：Miyo 的 Tauri 2 工程实践

> 技术拆解文，主投掘金 / InfoQ / dev.to（英文版可译）。配图：docs/imags/architecture.svg。

Miyo 是一个跨平台 API 调试工具：HTTP / WebSocket / SSE / GraphQL / gRPC 调试、Mock Server、断言测试、压测、CLI——全部塞进一个 **~10MB** 的安装包，冷启动不到 1 秒，常驻内存 ~40MB。同类的 Electron 应用普遍 300MB 起步。

这篇文章拆解我们是怎么做到的，以及为此付出的代价。

## 架构：三层隔离

```
Layer 1  前端      Vue 3 + TypeScript + Pinia（系统 WebView 渲染）
Layer 2  命令层    Tauri 2 插件（40+ IPC 命令，独立 Cargo workspace）
Layer 3  领域层    17 个纯 Rust crate（无 UI 依赖）
```

关键是 Layer 3 的**纪律**：17 个 crate（miyo-http / miyo-storage / miyo-test / miyo-grpc / miyo-mock / miyo-agent …）没有一个依赖 Tauri。命令层只是薄薄一层参数转换。这带来三个直接好处：

1. 领域逻辑可以脱离 GUI 做集成测试（我们有一个 10 秒内跑完的端到端冒烟：建项目 → 建环境 → 建接口 → 启动 Mock → 发请求 → 断言 → 落库）；
2. `miyo-test` 引擎被 CLI 无头复用，GUI 和 CI 跑的是同一套代码；
3. 编译并行度好，改前端不用重编 Rust。

## 体积：10MB 是怎么省出来的

**系统 WebView 是最大的红利**。macOS 用 WKWebView，Windows 用 WebView2，Linux 用 WebKitGTK——渲染层 0 字节打包。对比 Electron 每个应用自带一份 Chromium + Node（150MB+ 起步）。

**Rust 侧的裁剪**：

- tokio 按特性裁剪（`rt-multi-thread / macros / net / time / io-util / fs / sync`），不开 full；
- sqlx 只开 sqlite；reqwest 精简特性；
- release profile：`lto = "thin"` + `codegen-units = 1` + 符号剥离；
- 前端构建做了 vendor 分包（codemirror / chart 单独 chunk），首屏 chunk 很小；
- 字体只打 latin 子集 + woff2，从 760KB 裁到 1/3。

真正的大头是 rusqlite/sqlx 的 SQLite、reqwest 的 TLS 栈（rustls）和 axum（mock server 直接嵌在应用里）。10MB 是在这些都"要"的前提下省出来的。

## 本地优先的完整闭环：SQLite + AES-256-GCM

数据层选了 SQLite（sqlx），一切都在一个 `.db` 文件里——用户拷走目录就是完整备份。

敏感值（环境变量、签名密钥）**落库前**用 AES-256-GCM 加密：随机 32 字节主密钥存 `{数据目录}/master.key`（权限 0600），每条记录独立 12 字节 nonce，密文格式 `base64(nonce):base64(ciphertext||tag)`。解密失败会显式报错而不是当明文继续——防止静默丢数据。

数据目录还有一套"自举"设计：bootstrap 文件放在固定位置记录覆盖路径，用户可以在设置页改数据目录，主密钥与库文件保证同树——用户整目录拷贝永远不会漏掉密钥。

## 命令层的两条契约测试

Tauri 的 IPC 有两个经典事故：命令注册了但权限清单漏了（运行时静默被拒），IPC 响应体用了 camelCase 导致前端读到 undefined。我们都真踩过，然后写成了**双向契约测试**：

- `permissions_contract_test`：`generate_handler!` 注册的命令 ↔ `permissions/default.toml` 白名单 ↔ build.rs 的 COMMANDS 清单，三处逐一对齐，漂移即红；
- 序列化契约：IPC 响应体强制 snake_case 字段，用测试锁定。

测试的假阳性为零，因为它们断言的是"产物"而不是"意图"。

## 数据库迁移：给"裸奔"上保险

早期版本的迁移是"裸跑"——迁移失败，库文件可能停在中间态。现在 `init_db` 的流程是：

```
连接 → （非新库）WAL checkpoint + 整库快照到 snapshots/（保留 5 份）→ 跑迁移 → PRAGMA integrity_check
```

失败可回滚，自检不过会报错并指引恢复路径。另一个教训是 **sqlx 迁移文件的校验和不可变**——我们曾经在一次批量重构里改了已应用迁移文件的注释，所有老库启动直接失败。这条现在写进了贡献规范：迁移文件以新编号追加，永不修改。

## 稳定性工程的其他细节

- **异步壳里的同步 IO** 是重灾区：整库 copy、快照写盘、日志读取全部进 `spawn_blocking`，审查清单里有一条"grep std::fs + async fn"；
- **事件监听的卸载竞态**：Tauri 的 `listen()` 是异步注册，路由切走后才 resolve 的监听器要靠 disposed 标志兜底解绑；
- **流式数据的上限纪律**：gRPC 流消息 500 条截断、响应体 20MB 上限、日志 tail 512KB——所有"用户可控体量"的缓冲都有界；
- 请求按接口 id 分桶 + requestId 取消：切页签永远不会串响应。

## 值得复用的三个结论

1. **Tauri 2 已经能扛正经生产力应用**。代价是你要自己补生态：版本契约、权限契约、迁移保险——这些测试写一次受益整个生命周期；
2. **领域层与壳分离不是洁癖，是复用**。我们的 CLI 没有写一行"运行测试"的新逻辑，因为引擎一开始就在独立 crate 里；
3. **体积是设计出来的，不是配置出来的**。特性裁剪、依赖审计、分包，每一项都只省几百 KB——加起来就是 10MB 和 300MB 的差距。

仓库完全开源（Apache-2.0）：[github.com/weihubeats/Miyo](https://github.com/weihubeats/Miyo)，架构图和 459 个测试都在里面，欢迎来拆。
