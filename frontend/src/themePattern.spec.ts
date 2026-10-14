/**
 * 主题写法回归测试：组件内禁止任何 `html[data-theme]` 选择器。
 *
 * - `:global(html[data-theme=...])` 形式经 Vite 构建后丢失（产物中无对应规则），
 *   导致浅色主题静默失效（侧边栏 / 右键菜单曾全黑）；
 * - 裸 `html[data-theme=...]` 形式虽不丢，但属组件级主题覆写，割裂
 *   「主题差异一律走 style.css CSS 变量（深/浅各一套）」的单源约束。
 * 两种形式一律收敛为 style.css 中的成对 token，组件只消费 var()。
 */
import { describe, expect, it } from 'vitest'
import { readdirSync, readFileSync, statSync } from 'node:fs'
import { join } from 'node:path'

function vueFiles(dir: string): string[] {
  const out: string[] = []
  for (const name of readdirSync(dir)) {
    const p = join(dir, name)
    if (statSync(p).isDirectory()) {
      if (name === 'node_modules' || name === 'dist') continue
      out.push(...vueFiles(p))
    } else if (name.endsWith('.vue')) {
      out.push(p)
    }
  }
  return out
}

describe('主题写法', () => {
  it('不存在组件级 html[data-theme] 覆写（:global 构建丢弃；裸选择器亦禁）', () => {
    const offenders: string[] = []
    for (const file of vueFiles(join(process.cwd(), 'src'))) {
      const text = readFileSync(file, 'utf8')
      if (text.includes(':global(html[') || text.includes('html[data-theme')) offenders.push(file)
    }
    expect(offenders).toEqual([])
  })
})
