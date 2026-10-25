#!/usr/bin/env bash
# 录制 README/官网用的快速上手演示 GIF：项目列表 → 工作台 → 选中接口 → 发送 → 响应。
#
# 前置条件：
#   1. cliclick（brew install cliclick）——仅使用鼠标点击（规避中文输入法）；
#   2. Python3 + Pillow（装配 GIF 帧）；
#   3. 开发应用已在运行：cd frontend && npm run tauri dev（另开终端）。
#
# 稳定性设计：
#   - 每步点击后用 AX（辅助功能）轮询「目标元素已出现」才走下一步，
#     未出现则重试点击——固定 sleep 追不上可变渲染时间，这是早期脚本的
#     主要翻车点；
#   - 后端用脚本自起的 node HTTP 服务顶在环境默认 Base URL 端口（4010），
#     并做可达性预检（绑定失败会静默死亡）；
#   - 录制前点「全部折叠」归一化接口树（展开状态会持久化）。
#
# 用法：bash scripts/make_demo_gif.sh
# 产物：docs/imags/demo-debug.gif（720×400，逐帧步进式演示）

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="$ROOT/docs/imags/demo-debug.gif"
FRAMES="$(mktemp -d)"
DEMO_PID=""

cleanup() {
  if [[ -n "${DEMO_PID:-}" ]]; then kill "$DEMO_PID" 2>/dev/null || true; fi
  rm -rf "$FRAMES"
}
trap cleanup EXIT

say() { echo "==> $*"; }

# 清理上一次运行残留在 4010 的演示服务
lsof -t -iTCP:4010 -sTCP:LISTEN 2>/dev/null | xargs kill 2>/dev/null || true
sleep 0.3

# ---------- AX 轮询：等待名称为 $1 的元素出现（$2 = 超时秒） ----------
wait_el() {
  local name="$1" timeout="${2:-10}" script
  script="tell application \"System Events\" to tell process \"miyo\"
    set els to entire contents of window 1
    repeat with e in els
      try
        if name of e is \"$name\" then return true
      end try
    end repeat
    return false
  end tell"
  local deadline=$((SECONDS + timeout))
  while (( SECONDS < deadline )); do
    if osascript -e "$script" 2>/dev/null | grep -q true; then return 0; fi
    sleep 1
  done
  return 1
}

shot() { screencapture -R0,25,1440,800 "$FRAMES/$1.png"; }

# ---------- 后端：环境默认 Base URL 端口（4010）上起确定性 JSON 服务 ----------
node -e '
require("http").createServer((q, s) => {
  s.setHeader("content-type", "application/json");
  s.end(JSON.stringify({ data: [{ id: 1, name: "miyo" }, { id: 2, name: "demo" }] }));
}).listen(4010); // 不绑 host：双栈监听，localhost(::1) 与 127.0.0.1 都可达
' 2>/dev/null &
DEMO_PID=$!
for attempt in 1 2 3 4 5; do
  if curl -sf -m 2 http://127.0.0.1:4010/users >/dev/null; then
    say "演示后端就绪（127.0.0.1:4010）"
    break
  fi
  if [[ $attempt == 5 ]]; then echo "✗ 演示后端未就绪，中止" >&2; exit 1; fi
  sleep 1
done

# ---------- 前置：窗口定位 ----------
osascript -e 'tell application "System Events" to tell process "miyo" to set frontmost to true' \
          -e 'tell application "System Events" to tell process "miyo" to set position of window 1 to {0, 25}' \
          -e 'tell application "System Events" to tell process "miyo" to set size of window 1 to {1440, 800}'
sleep 1

say "帧 1：项目列表（首页仪表盘）"
shot f1

say "帧 2：进入工作台（点击用户服务项目卡，AX 确认树出现）"
for attempt in 1 2 3; do
  cliclick c:384,545
  if wait_el "账号管理" 12; then break; fi
  say "  重试项目卡点击（$attempt）"
done
wait_el "账号管理" 5 || { echo "✗ 工作台未就绪" >&2; exit 1; }
shot f2

say "归一化接口树（全部折叠）"
cliclick c:268,141
sleep 1.2

say "帧 3：展开接口树（账号管理，AX 确认接口行出现）"
for attempt in 1 2 3; do
  cliclick c:68,179
  if wait_el "用户列表" 8; then break; fi
  say "  重试展开点击（${attempt}）"
done
wait_el "用户列表" 5 || { echo "✗ 接口树未展开" >&2; exit 1; }
shot f3

say "帧 4：选中接口（用户列表，AX 确认发送按钮出现）"
for attempt in 1 2 3; do
  cliclick c:110,209
  if wait_el "发送" 8; then break; fi
  say "  重试接口点击（${attempt}）"
done
wait_el "发送" 5 || { echo "✗ 编辑器未就绪" >&2; exit 1; }
sleep 1
shot f4

say "选择 Base URL（草稿里持久化的旧值 localhost → 切到 4010 环境）"
cliclick c:466,219
sleep 1.5
cliclick c:478,321
sleep 1.5

say "帧 5：发送请求（AX 确认响应格式化按钮出现 = 请求成功）"
for attempt in 1 2 3; do
  cliclick c:1032,219
  if wait_el "格式化" 15; then break; fi
  say "  重试发送（${attempt}）"
done
wait_el "格式化" 10 || { echo "✗ 响应未出现" >&2; exit 1; }
sleep 0.8
shot f5

say "帧 6：镜头停在响应 JSON"
sleep 1.5
shot f6

# ---------- 装配 ----------
say "装配 GIF → $OUT"
python3 - "$FRAMES" "$OUT" <<'PY'
import sys, glob
from PIL import Image
frames_dir, out = sys.argv[1], sys.argv[2]
files = sorted(glob.glob(f"{frames_dir}/f*.png"))
assert files, "没有捕获到任何帧"
frames = []
for f in files:
    im = Image.open(f).convert("RGB").resize((720, 400), Image.LANCZOS)
    frames.append(im)
durations = [1400, 1000, 900, 1100, 900, 2200][: len(frames)]
if len(durations) < len(frames):
    durations += [1800] * (len(frames) - len(durations))
frames[0].save(out, save_all=True, append_images=frames[1:], duration=durations, loop=0)
print(f"frames={len(frames)} -> {out}")
PY
say "完成：$OUT"
