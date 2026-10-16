/**
 * TestCaseDrawer 单测：打开回填 / 修改后保存 / 原地运行（结果留在抽屉内，不切 Tab）/
 * 导出代码（复用 CodeExportMenu：注入解析后 URL + BodySpec）。
 */
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { nextTick } from 'vue'
import TestCaseDrawer from './TestCaseDrawer.vue'
import CodeExportMenu from './CodeExportMenu.vue'
import { useLocaleStore } from '../stores/locale'
import { useWorkspaceStore } from '../stores/workspace'
import type { Environment, TestCase } from '../types/foxApi'

function makeCase(): TestCase {
  return {
    id: 'c1',
    request_id: 'ep-1',
    name: '内部划转-SGB',
    category: '正向',
    method: 'POST',
    url_path: '/funds/transfer',
    params: [{ key: 'env', value: 'prod', enabled: true, description: '' }],
    headers: [{ key: 'X-Trace', value: 'on', enabled: true, description: '' }],
    body_type: 'json',
    body_content: '{"amount":100}',
    last_run_status: 'Untested',
    created_at: '2026-01-01T00:00:00.000Z',
  }
}

function mountDrawer(overrides: { open?: boolean; testCase?: TestCase | null } = {}) {
  const onRun = vi.fn()
  const onSave = vi.fn()
  const resolveUrl = vi.fn((path: string) => `https://api.test${path}`)
  const wrapper = mount(TestCaseDrawer, {
    props: {
      open: overrides.open ?? true,
      endpointId: 'ep-1',
      testCase: overrides.testCase ?? makeCase(),
      resolveUrl,
      onRun,
      onSave,
    },
  })
  return { wrapper, onRun, onSave, resolveUrl }
}

describe('TestCaseDrawer', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    // 文案断言锁定中文（jsdom 默认语言为英文，跟随系统会解析出英文）
    useLocaleStore().setMode('zh')
    document.body.innerHTML = ''
  })

  it('打开时回填用例内容（名称 / 分组 / Path / Body）', async () => {
    mountDrawer()
    const inputs = Array.from(document.querySelectorAll<HTMLInputElement>('.drw-input'))
    expect(inputs.map((i) => i.value)).toEqual(
      expect.arrayContaining(['内部划转-SGB', '/funds/transfer']),
    )
    // Body 位于请求参数 Tab（Params/Headers/Body）内，由 CodeMirror 承载
    const bodyTab = Array.from(document.querySelectorAll('.drw-tab')).find((b) => b.textContent === 'Body')
    ;(bodyTab as HTMLButtonElement).click()
    await vi.waitFor(() => {
      const content = document.querySelector<HTMLElement>('.cm-content')
      expect(content?.textContent?.replace(/\s+/g, '')).toBe('{"amount":100}')
    })
  })

  it('Method 联动：POST 默认 Body Tab + 自动补 Content-Type；GET 默认 Params Tab + Body 提示', async () => {
    mountDrawer()
    // POST 用例 → 默认落到 Body Tab，且自动补 Content-Type: application/json
    await vi.waitFor(() => {
      const active = document.querySelector<HTMLElement>('.drw-tab.active')
      expect(active?.textContent).toBe('Body')
    })
    expect(document.querySelector('.cm-content')).toBeTruthy()
    // 自动补 Content-Type: application/json（切到 Headers Tab，输入框值可见）
    const headersTab = Array.from(document.querySelectorAll('.drw-tab')).find((b) => b.textContent === 'Headers')
    ;(headersTab as HTMLButtonElement).click()
    await vi.waitFor(() => {
      const keys = Array.from(
        document.querySelectorAll<HTMLInputElement>('.kvt-row .kvt-key'),
      ).map((i) => i.value)
      expect(keys).toContain('Content-Type')
    })
    // 切到 GET → 默认 Params Tab，Body Tab 出现「不携带 Body」提示
    const methodTrigger = document.querySelector<HTMLElement>('.drw-method .cs-trigger')
    ;(methodTrigger as HTMLElement).click()
    await vi.waitFor(() => {
      const opt = Array.from(document.querySelectorAll('.cs-opt')).find(
        (el) => el.textContent?.trim() === 'GET',
      )
      expect(opt).toBeTruthy()
      ;(opt as HTMLElement).click()
    })
    await vi.waitFor(() => {
      const active = document.querySelector<HTMLElement>('.drw-tab.active')
      expect(active?.textContent).toBe('Params')
    })
    const bodyTab = Array.from(document.querySelectorAll('.drw-tab')).find((b) => b.textContent === 'Body')
    ;(bodyTab as HTMLButtonElement).click()
    await vi.waitFor(() => {
      expect(document.body.textContent).toContain('GET 请求通常不携带 Body')
    })
  })

  it('请求区 / 响应区存在拖拽分割条，双击恢复 50% 比例', async () => {
    mountDrawer()
    const splitter = document.querySelector<HTMLElement>('.drw-splitter')
    expect(splitter).toBeTruthy()
    const reqSec = document.querySelector<HTMLElement>('.drw-req-sec')
    expect(reqSec?.style.flexBasis).toBe('55%')
    ;(splitter as HTMLElement).dispatchEvent(new MouseEvent('dblclick'))
    await new Promise((r) => setTimeout(r, 0))
    expect(reqSec?.style.flexBasis).toBe('50%')
  })

  it('「保存修改」携带完整配置（含名称 / 分组），保存成功后关闭', async () => {
    const { wrapper, onSave } = mountDrawer()
    const saveBtn = Array.from(document.querySelectorAll('.drw-foot button')).find(
      (b) => b.textContent?.trim() === '保存修改',
    ) as HTMLButtonElement
    expect(saveBtn.disabled).toBe(false)
    saveBtn.click()
    await vi.waitFor(() => {
      expect(onSave).toHaveBeenCalledWith(
        expect.objectContaining({
          name: '内部划转-SGB',
          category: '正向',
          method: 'POST',
          urlPath: '/funds/transfer',
          bodyType: 'json',
          bodyContent: '{"amount":100}',
        }),
      )
    })
    expect(wrapper.emitted('update:open')).toBeTruthy()
  })

  it('「立即运行」调用 onRun 并展示响应状态与 Body（Success）', async () => {
    const { onRun } = mountDrawer()
    onRun.mockResolvedValue({
      status: 200,
      headers: [],
      body: '{"ok":true}',
      content_type: 'application/json',
      duration_ms: 320,
      size_bytes: 10,
      truncated: false,
    })
    const runBtn = Array.from(document.querySelectorAll('.drw-foot button')).find(
      (b) => b.textContent?.includes('立即运行'),
    ) as HTMLButtonElement
    runBtn.click()
    await vi.waitFor(() => {
      expect(onRun).toHaveBeenCalledWith(
        expect.objectContaining({ method: 'POST', urlPath: '/funds/transfer' }),
      )
    })
    await vi.waitFor(() => {
      expect(document.body.textContent).toContain('200 OK')
      expect(document.body.textContent).toContain('320ms')
      expect(document.body.textContent).toContain('"ok": true')
    })
  })

  it('顶部用例基本信息右侧提供环境选择器，切换写回激活环境', async () => {
    const env: Environment = {
      id: 'e1',
      project_id: 'p1',
      name: '开发环境',
      base_url: 'http://127.0.0.1:4010',
      variables: [],
      created_at: '2026-01-01T00:00:00.000Z',
      updated_at: '2026-01-01T00:00:00.000Z',
    }
    const store = useWorkspaceStore()
    store.environments = [env]
    store.activeEnvId = 'e1'
    const setEnv = vi.spyOn(store, 'setEnvironment').mockResolvedValue(undefined)
    const { wrapper } = mountDrawer()

    // 与顶栏同一个共享选择器（EnvironmentSelector，solo 外观）：收起态 = 状态点 + 环境名
    const trigger = document.querySelector<HTMLElement>('.drw-sec-head .drw-env-select .cs-trigger')
    expect(trigger).toBeTruthy()
    expect(trigger!.querySelector('.edot')).toBeTruthy()
    expect(trigger!.textContent).toContain('开发环境')
    // manage=false：不渲染环境管理弹窗（遮罩盖得住 Modal，且不拉管理大 chunk）
    expect(wrapper.findComponent({ name: 'EnvironmentManager' }).exists()).toBe(false)

    trigger!.click()
    await nextTick()
    const pop = document.querySelector('.cs-pop.env-pop')!
    expect(pop).toBeTruthy()
    // 格式与顶栏一致：无环境 + 各环境（名称行）
    const rows = Array.from(pop.querySelectorAll<HTMLElement>('.cs-pop-list .cs-opt'))
    expect(rows.map((r) => r.querySelector('.env-opt-name')?.textContent?.trim())).toEqual([
      '无环境',
      '开发环境',
    ])
    // 当前选中项高亮 + 行尾 ✓
    expect(pop.querySelector('.cs-opt.sel .env-opt-name')?.textContent?.trim()).toBe('开发环境')
    expect(pop.querySelector('.cs-opt.sel .cs-opt-check svg')).toBeTruthy()
    // 抽屉遮罩(120) 高于 Modal(100)：抽屉内不给环境管理入口（底栏不渲染）
    expect(pop.querySelector('.es-pop-footer')).toBeNull()

    // 切到「无环境」→ 与顶栏同源写回（setEnvironment(null)）
    rows[0].click()
    await nextTick()
    expect(setEnv).toHaveBeenCalledWith(null)

    // 再切回指定环境 → 写回环境 id
    trigger!.click()
    await nextTick()
    Array.from(
      document.querySelectorAll<HTMLElement>('.cs-pop.env-pop .cs-pop-list .cs-opt'),
    )[1].click()
    await nextTick()
    expect(setEnv).toHaveBeenCalledWith('e1')
  })

  it('底部操作栏只保留核心动作（环境控件已移至顶部）', () => {
    mountDrawer()
    // 抽屉 Teleport 到 body，DOM 断言走 document（与既有用例一致）
    const foot = document.querySelector('.drw-foot')
    expect(foot).toBeTruthy()
    // 无环境徽章 / 选择器残留
    expect(foot!.querySelector('.env-badge')).toBeNull()
    expect(foot!.querySelector('.drw-env-select')).toBeNull()
    // 核心动作齐全：导出代码 / 立即运行 / 取消 / 保存
    const footText = foot!.textContent ?? ''
    expect(footText).toContain('导出代码')
    expect(footText).toContain('立即运行')
    expect(footText).toContain('取消')
    expect(footText).toContain('保存修改')
    // 选择器在顶部基本信息卡内
    expect(document.querySelector('.drw-sec-head .drw-env-select')).toBeTruthy()
  })

  it('运行失败展示 Failed 与错误信息', async () => {
    const { onRun } = mountDrawer()
    onRun.mockResolvedValue({
      status: 500,
      headers: [],
      body: 'boom',
      content_type: 'text/plain',
      duration_ms: 12,
      size_bytes: 4,
      truncated: false,
    })
    const runBtn = Array.from(document.querySelectorAll('.drw-foot button')).find(
      (b) => b.textContent?.includes('立即运行'),
    ) as HTMLButtonElement
    runBtn.click()
    await vi.waitFor(() => {
      expect(document.body.textContent).toContain('500 Internal Error')
      expect(document.body.textContent).toContain('12ms')
    })
  })

  it('名称 / Path 为空时保存按钮禁用', async () => {
    const c = makeCase()
    c.name = ''
    mountDrawer({ testCase: c })
    const saveBtn = Array.from(document.querySelectorAll('.drw-foot button')).find(
      (b) => b.textContent?.trim() === '保存修改',
    ) as HTMLButtonElement
    expect(saveBtn.disabled).toBe(true)
  })

  it('导出代码：注入解析后 URL + BodySpec（贴底向上弹 + 左对齐右展开），Path 为空时置灰', async () => {
    const { wrapper, resolveUrl } = mountDrawer()
    const menu = wrapper.findComponent(CodeExportMenu)
    expect(menu.exists()).toBe(true)
    expect(menu.props('placement')).toBe('top')
    // 弹出层左对齐向右展开，留在抽屉面板内（与 CustomSelect 下拉方向一致）
    expect(menu.props('align')).toBe('left')
    expect(resolveUrl).toHaveBeenCalledWith('/funds/transfer')
    expect(menu.props('method')).toBe('POST')
    expect(menu.props('url')).toBe('https://api.test/funds/transfer')
    // body_type + 内容还原为 BodySpec（与运行时 restoreBody 同源）
    expect(menu.props('body')).toEqual({ mode: 'json', raw: '{"amount":100}' })
    // 用例无独立鉴权，导出与运行一致（auth none）
    expect(menu.props('auth')).toEqual({ type: 'none' })
    expect(menu.props('disabled')).toBe(false)

    // Path 清空 → URL 清空且导出置灰
    const pathInput = document.querySelector<HTMLInputElement>('.drw-path-input')!
    pathInput.value = ''
    pathInput.dispatchEvent(new Event('input'))
    await vi.waitFor(() => {
      expect(menu.props('url')).toBe('')
      expect(menu.props('disabled')).toBe(true)
    })
  })

  it('请求 Body 支持查找：FindBar 计数 + 输入不抢光标 + Esc 关闭', async () => {
    // jsdom 缺少 Range#getClientRects，CodeMirror 无法聚焦——补上最小桩
    // 以还原浏览器行为，否则「抢光标」bug 在测试里不可见。
    Object.defineProperty(window.Range.prototype, 'getClientRects', {
      configurable: true,
      value: () => [],
    })
    mountDrawer()
    // POST 用例默认落在 Body Tab，编辑器已挂载
    await vi.waitFor(() => {
      expect(document.querySelector('.cm-content')).toBeTruthy()
    })
    const toggle = document.querySelector<HTMLElement>('.drw-icon-btn')
    expect(toggle).toBeTruthy()
    ;(toggle as HTMLElement).click()
    await vi.waitFor(() => {
      expect(document.querySelector('.findbar')).toBeTruthy()
    })
    const input = document.querySelector<HTMLInputElement>('.findbar-input')!
    input.value = 'amount'
    input.dispatchEvent(new Event('input'))
    // 160ms 防抖后计数为 1 / 1，且编辑器内实时高亮（无需回车）
    await vi.waitFor(() => {
      expect(document.querySelector('.findbar-count')?.textContent).toContain('1 / 1')
    })
    await vi.waitFor(() => {
      expect(document.querySelector('.cm-searchMatch')).toBeTruthy()
    })
    // 输入过程中焦点必须留在搜索框（FindBar 挂载即自动聚焦，编辑器不得抢光标）
    await new Promise((r) => setTimeout(r, 350))
    expect(document.activeElement).toBe(input)
    // 按 Enter（下一个）才显式跳转到编辑器
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }))
    await vi.waitFor(() => {
      expect(document.querySelector('.cm-editor')?.contains(document.activeElement)).toBe(true)
    })
    // 无命中时显示无匹配
    input.value = 'no-such-body-zzz'
    input.dispatchEvent(new Event('input'))
    await vi.waitFor(() => {
      expect(document.querySelector('.findbar-count')?.textContent).toContain('无匹配')
    })
    // Esc 关闭查找条
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }))
    await vi.waitFor(() => {
      expect(document.querySelector('.findbar')).toBeNull()
    })
  })
})