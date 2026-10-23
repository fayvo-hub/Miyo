# 自动更新（tauri-plugin-updater）配置指南

应用内「关于 → Check for Updates」走 Tauri 2 updater 插件：
`tauri.conf.json` 的 `plugins.updater.endpoints` 指向 GitHub Release 的
`latest.json`，客户端校验 minisign 签名后下载安装并重启。

## 一次性配置（维护者）

### 1. 生成签名密钥对

```bash
cd frontend
npm run tauri -- signer generate -w ~/.tauri/miyo-updater.key
```

- 私钥：`~/.tauri/miyo-updater.key`（**丢失后已发布用户将无法静默升级，务必备份**）
- 公钥：`~/.tauri/miyo-updater.key.pub`

### 2. 公钥写入应用配置

把公钥内容替换 `frontend/src-tauri/tauri.conf.json` 里的占位符：

```json
"plugins": { "updater": { "pubkey": "REPLACE_WITH_TAURI_SIGNER_PUBKEY" } }
```

### 3. 私钥配置到 GitHub Secrets（仓库 Settings → Secrets → Actions）

| Secret | 值 |
|---|---|
| `TAURI_SIGNING_PRIVATE_KEY` | 私钥文件完整内容 |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | 生成私钥时设置的密码（未设密码则留空/不配） |

## 发布流程（自动）

打 tag（如 `v0.0.3`）触发 `release.yml`：

1. 各平台构建时注入签名私钥，产出 updater 产物（macOS `.app.tar.gz`、
   Linux `.AppImage`、Windows `-setup.exe`）及对应 `.sig` 签名文件；
2. release job 汇总产物，生成 `latest.json`（版本号、各平台下载 URL、签名）
   并附到 GitHub Release；
3. 客户端「检查更新」拉取 `releases/latest/download/latest.json` 对比版本。

> 注意：**未配置 `TAURI_SIGNING_PRIVATE_KEY` secret 前不要打 tag 发版**，
> 构建签名产物时会失败。

## 版本号

发版前同步三处版本：`frontend/package.json`、`frontend/src-tauri/tauri.conf.json`、
`frontend/src-tauri/Cargo.toml`（`miyo` 包）。updater 以 `tauri.conf.json` 的
`version` 为当前版本，与 `latest.json` 里的 `version` 比较（semver 语义）。

## 开发验证（无需改版号/重启）

开发版（`import.meta.env.DEV`）的设置 → 通用页底部有「更新调试」区：

- **模拟新版**：输入版本号（如 `9.9.9`），走与真实检查相同的去重/跳过/通知链
  （toast + 设置齿轮小红点 + 关于弹窗承接全生效；模拟数据不可安装）；
- **模拟失败一次**：下一次检查强制失败，验证静默 + 30 分钟重试已安排；
- **立即检查**：绕过节流与启动延迟，马上跑一次真实检查；
- **清除锁存**：清空检查节流与提醒记录；下方状态行实时显示各锁存值。

生产包无任何入口调用这些 API（`grep debugSimulateUpdate` 仅命中设置调试区与单测）。

## 客户端提醒策略（`useAutoUpdate`）

- 启动约 8 秒后检查一次，之后每 6 小时轮询；距上次**成功**检查不足 6 小时则跳过
  （多窗口/频繁重启不重复打扰）；
- 检查失败不计入节流，30 分钟后重试一次（网络抖动不再静默顺延 6 小时）；
- 同版本每天最多提醒一次（toast 15 秒，错过第二天可恢复）；跳过的版本保持静默，
  直到出现更新的版本；
- 有待安装更新时设置齿轮显示常驻小红点，设置内更新行可直达关于弹窗一键安装；
  安装/跳过后熄灭。
