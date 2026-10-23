/**
 * 路径变量（URL path 传参）公共逻辑 —— 抽取与代入的单一真源。
 *
 * 三种写法都要认，且与后端 `miyo_core::util` 对齐：
 * - `{{id}}`：本应用变量语法（调试页 / 环境变量同款），可被环境解析；
 * - `{id}`：OpenAPI / Swagger 路径模板；
 * - `:id`：Postman 路径模板。
 *
 * 抽取前先剥掉 origin 与 query/hash，避免 `https://x:8080` 的端口、
 * `user:pass@` 的口令被当成 `:name` 占位。
 */

/** 变量名的合法形态：字母/下划线/`$` 开头，后续允许字母数字、`_`、`$`、`-`。 */
const NAME = '[A-Za-z_$][\\w$-]*'
const DOUBLE = new RegExp(`\\{\\{\\s*(${NAME})\\s*\\}\\}`, 'g')
const SINGLE = new RegExp(`\\{\\s*(${NAME})\\s*\\}`, 'g')
const COLON = new RegExp(`(?<![\\w]):(${NAME})`, 'g')

/** 从路径抽取占位名（按出现顺序去重）。 */
export function extractPathVarNames(path: string): string[] {
  const cleaned = path
    .replace(/^[a-zA-Z][a-zA-Z0-9+.-]*:\/\/[^/?#]*/, '')
    .replace(/[?#].*$/, '')
  const found: { i: number; name: string }[] = []
  for (const m of cleaned.matchAll(DOUBLE)) {
    found.push({ i: m.index ?? 0, name: m[1] })
  }
  // 单花括号扫描时先把 `{{...}}` 用等长空白遮掉，避免 `{{id}}` 被拆成 `{id`。
  const noDouble = cleaned.replace(DOUBLE, (s) => ' '.repeat(s.length))
  for (const m of noDouble.matchAll(SINGLE)) {
    found.push({ i: m.index ?? 0, name: m[1] })
  }
  for (const m of noDouble.matchAll(COLON)) {
    found.push({ i: m.index ?? 0, name: m[1] })
  }
  found.sort((a, b) => a.i - b.i)
  const out: string[] = []
  const seen = new Set<string>()
  for (const f of found) {
    if (f.name && !seen.has(f.name)) {
      seen.add(f.name)
      out.push(f.name)
    }
  }
  return out
}

/** 代入所需的最小行形态（`{key,value,enabled}`，与 KeyValue 兼容）。 */
export interface PathVarValue {
  key: string
  value: string
  enabled?: boolean
}

/**
 * 路径变量代入：镜像后端 `replace_path_variables`（`{key}` 与 `{{key}}` 两种写法，
 * 长 key 先替换避免前缀冲突）。仅替换「已启用且取值非空」的行——未配置时路径原样
 * 发送，保持历史行为不变。
 */
export function applyPathVars(path: string, vars: PathVarValue[] | undefined): string {
  const rows = (vars ?? [])
    .filter((r) => r.enabled !== false && r.key && r.value)
    .sort((a, b) => b.key.length - a.key.length)
  let out = path
  for (const row of rows) {
    out = out.split(`{{${row.key}}}`).join(row.value).split(`{${row.key}}`).join(row.value)
  }
  return out
}