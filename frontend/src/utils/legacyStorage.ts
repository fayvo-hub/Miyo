/**
 * 品牌更名（RustFox → Miyo）的一次性 localStorage 键迁移。
 *
 * 老版本把主题 / 语言 / 快捷键 / 更新提醒 / 打开页签 / 项目 Base URL /
 * GraphQL 历史 / URL 缓存等偏好存在 `rustfox*` 前缀键下，更名后键前缀全部
 * 换成 `miyo*`。启动最早阶段调用一次：旧键存在且新键缺失时搬移并删除旧键；
 * 新键已存在（已迁移 / 更名后新写）时跳过，绝不覆盖新值。
 */

/** 精确键映射（旧 → 新）。 */
const LEGACY_KEY_MAP: ReadonlyArray<readonly [string, string]> = [
  ['rustfox.theme.mode', 'miyo.theme.mode'],
  ['rustfox.locale.mode', 'miyo.locale.mode'],
  ['rustfox:shortcut-bindings', 'miyo:shortcut-bindings'],
  ['rustfox:update:last-check', 'miyo:update:last-check'],
  ['rustfox:update:notified-version', 'miyo:update:notified-version'],
  ['rustfox:update:notified-at', 'miyo:update:notified-at'],
  ['rustfox:update:skipped-version', 'miyo:update:skipped-version'],
  ['rustfox.open-projects', 'miyo.open-projects'],
  ['rustfox_graphql_last_url', 'miyo_graphql_last_url'],
  ['rustfox_graphql_history', 'miyo_graphql_history'],
]

/** 前缀键映射（按项目隔离的动态键，如 `rustfox.base-url.<projectId>`）。 */
const LEGACY_KEY_PREFIXES: ReadonlyArray<readonly [string, string]> = [
  ['rustfox.base-url.', 'miyo.base-url.'],
]

export function migrateLegacyStorageKeys(): void {
  for (const [legacyKey, newKey] of LEGACY_KEY_MAP) {
    const legacyValue = localStorage.getItem(legacyKey)
    if (legacyValue === null || localStorage.getItem(newKey) !== null) {
      localStorage.removeItem(legacyKey)
      continue
    }
    localStorage.setItem(newKey, legacyValue)
    localStorage.removeItem(legacyKey)
  }
  // 遍历中删除当前项时索引不前进（其余情况前进），避免跳键
  for (let i = 0; i < localStorage.length; ) {
    const key = localStorage.key(i)
    if (key === null) {
      break
    }
    const mapping = LEGACY_KEY_PREFIXES.find(([p]) => key.startsWith(p))
    if (mapping === undefined) {
      i += 1
      continue
    }
    const [legacyPrefix, newPrefix] = mapping
    const newKey = newPrefix + key.slice(legacyPrefix.length)
    if (localStorage.getItem(newKey) === null) {
      const legacyValue = localStorage.getItem(key)
      if (legacyValue !== null) {
        localStorage.setItem(newKey, legacyValue)
      }
    }
    localStorage.removeItem(key)
  }
}
