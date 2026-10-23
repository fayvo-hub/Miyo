/**
 * environment.ts：环境相关的前端工具（单一 Base URL / 结构化变量 / URL 拼接）。
 *
 * - envColorClass：按环境名称启发式归类颜色（开发绿 / 测试蓝 / 预发布琥珀 / 生产橙 / 全局紫）；
 * - envBaseUrl：环境声明的 Base URL（解析前的原始文本，保留 `{{变量}}`）；
 * - environmentVariableMap：结构化变量 → 扁平 `{{name}}` 注入表（enabled、本地值优先；
 *   环境 Base URL 自动以 `base_url` 注入，已有同名变量时不覆盖）；
 * - resolveRequestUrl：请求拼接核心 —— 最终 URL = 环境 Base URL + 相对路径；
 * - resolveVariables：镜像 miyo-core variable.rs 的 `{{name}}` 单层递归解析（深度上限 10）。
 */

import type { Environment } from '../types/miyoApi'

/** 环境名称 → 颜色类（映射类名，颜色值由调用方 scoped CSS 定义）。 */
export function envColorClass(name: string): string {
  const n = name.trim().toLowerCase()
  if (/(开发|development|dev)/.test(n)) return 'dev'
  if (/(测试|test|qa)/.test(n)) return 'test'
  if (/(预发布|staging|stage|pre)/.test(n)) return 'staging'
  if (/(生产|prod|production|live)/.test(n)) return 'prod'
  if (/(全局|global)/.test(n)) return 'global'
  return ''
}

/** 环境的 Base URL（解析前的原始文本，保留 `{{变量}}`）。 */
export function envBaseUrl(env: Environment | null | undefined): string {
  return env?.base_url?.trim() ?? ''
}

/** 变量生效值：本地覆盖值非空时优先，否则取远程 / 公共值。 */
export function effectiveVariable(v: { remote_value: string; local_value: string }): string {
  return v.local_value.trim() || v.remote_value.trim()
}

/** 结构化变量列表 → 扁平注入表（enabled 才注入、本地值优先、本地覆盖生效）。 */
export function variableListToMap(
  vars: { key: string; remote_value: string; local_value: string; enabled: boolean }[] | null | undefined,
): Record<string, string> {
  const out: Record<string, string> = {}
  for (const v of vars ?? []) {
    if (!v.enabled) continue
    const key = v.key.trim()
    if (!key || key.startsWith('{{') || key.startsWith('$')) continue
    const value = effectiveVariable(v)
    if (value) out[key] = value
  }
  return out
}

/**
 * 结构化变量 → 扁平注入表（enabled 才注入；本地值优先；未知键原样）。
 * 环境 Base URL 自动以 `base_url` 注入（已有同名变量时不覆盖），
 * 使 `{{base_url}}` 在旧语义下继续可用。
 */
export function environmentVariableMap(
  env: Environment | null | undefined,
): Record<string, string> {
  const out = variableListToMap(env?.variables)
  const base = envBaseUrl(env)
  if (base && !out.base_url) out.base_url = base
  return out
}

/** 规范化基础 URL：去掉尾部斜杠，避免与路径拼接出双斜杠（`https://x.com//posts`）。 */
export function normalizeBaseUrl(value: string): string {
  const s = value.trim()
  const stripped = s.replace(/\/+$/, '')
  // 保留协议完整性（避免 `https://` 被削成 `https:`）
  if (/^[a-zA-Z][a-zA-Z0-9+.-]*:\/+$/.test(s)) return s
  return stripped
}

/** 变量递归解析（镜像后端：单次扫描 + 深度上限；未知变量原样保留）。 */
export function resolveVariables(
  input: string,
  vars: Record<string, string>,
  depth = 0,
): string {
  if (depth >= 10) return input
  return input.replace(/\{\{\s*([^{}]+?)\s*\}\}/g, (full, name: string) => {
    const value = vars[name]
    if (value == null || value === '') return full
    return resolveVariables(value, vars, depth + 1)
  })
}

function isAbsolute(s: string): boolean {
  return s.startsWith('http://') || s.startsWith('https://')
}

/** 基址 + 相对路径拼接（与 miyo-core util::build_url 一致：完整 URL 直用，否则去斜杠后拼接）。 */
export function joinBaseUrl(base: string, path: string): string {
  if (isAbsolute(path)) return path
  const p = path.trim().replace(/^\/+/, '')
  const b = normalizeBaseUrl(base)
  if (!b) return `/${p}`
  return `${b}/${p}`
}

export interface ResolvedRequestUrl {
  /** 最终请求 URL。 */
  url: string
}

/**
 * 请求拼接核心：
 * 1. `path` 为完整 http(s) 地址 → 直接使用；
 * 2. 否则以环境 Base URL 为基址拼接相对路径（无基址时仅返回路径）。
 * 基址 / 路径中的 `{{变量}}` 以「环境变量 + 调用方变量」合并表解析。
 */
export function resolveRequestUrl(
  env: Environment | null | undefined,
  path: string,
  extraVars: Record<string, string> = {},
): ResolvedRequestUrl {
  const vars = { ...extraVars, ...environmentVariableMap(env) }
  const rendered = resolveVariables(path, vars)
  if (isAbsolute(rendered)) return { url: rendered }
  const base = resolveVariables(envBaseUrl(env), vars)
  return { url: joinBaseUrl(base, rendered) }
}
