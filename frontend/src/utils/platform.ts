/**
 * platform：运行平台探测（同步、零依赖）。
 * - 依据 navigator.userAgent 区分 macOS / Windows / Linux；
 * - isTauri 判定是否运行在 Tauri WebView 内（浏览器预览为 false）。
 * 仅用于窗口装饰差异（Windows 自绘窗口控制钮 / macOS 红绿灯留白），业务逻辑不得依赖。
 */
export type DesktopPlatform = 'macos' | 'windows' | 'linux' | 'other'

export function isTauri(): boolean {
  return !!(window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__
}

export function currentPlatform(): DesktopPlatform {
  const ua = navigator.userAgent
  if (ua.includes('Mac')) return 'macos'
  if (ua.includes('Win')) return 'windows'
  if (ua.includes('Linux')) return 'linux'
  return 'other'
}

/**
 * 是否由前端自绘窗口控制钮（最小化/最大化/关闭）：
 * Windows 下原生装饰被移除（见 src-tauri main.rs），需要自绘顶栏控制钮；
 * macOS 用系统红绿灯、Linux 保持原生标题栏，均不渲染。
 */
export function showWindowControls(): boolean {
  return isTauri() && currentPlatform() === 'windows'
}
