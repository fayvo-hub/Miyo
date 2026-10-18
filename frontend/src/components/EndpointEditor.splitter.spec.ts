/**
 * EndpointEditor 回归测试：请求区手动分割态（拖拽/折叠）不跨接口残留。
 *
 * 背景：EndpointEditor 单实例常驻，requestManual / growChrome 为组件级状态。
 * 在 A 接口手动拖过（或双击折叠）分割条后打开 B 接口，请求卡仍锁定旧固定高，
 * min-height:0 的 Body 面板被挤成一条细线、「请求设置」行 sticky 叠画其上
 * （用户实测）。修复后切接口时手动分割态归零，新接口回到 grow 贴内容布局。
 */
import { describe, expect, it, vi } from 'vitest'
import { mount } from '@vue/test-utils'
import { nextTick } from 'vue'
import { createPinia, setActivePinia } from 'pinia'
import EndpointEditor from './EndpointEditor.vue'
import { useWorkspaceStore } from '../stores/workspace'
import { useLocaleStore } from '../stores/locale'
import { makeDraft } from '../testUtils/draftFixture'
import { stubScrollIntoView } from '../testUtils/componentTest'
import type { Endpoint } from '../types/foxApi'

vi.mock('../composables/useFoxApi', () => ({
  useFoxApi: () => new Proxy({}, { get: () => () => Promise.resolve(null) }),
}))

const endpointA: Endpoint = makeDraft({ id: 'ep-a', name: '接口 A', method: 'POST', path: '/a' })
const endpointB: Endpoint = makeDraft({ id: 'ep-b', name: '接口 B', method: 'POST', path: '/b' })

async function mountEditor() {
  setActivePinia(createPinia())
  stubScrollIntoView()
  useLocaleStore().setMode('zh')
  const store = useWorkspaceStore()
  store.project = {
    id: 'proj-test-1',
    name: 'P',
    description: '',
    variables: {},
    created_at: '2026-01-01T00:00:00.000Z',
    updated_at: '2026-01-01T00:00:00.000Z',
  }
  store.endpoints = [endpointA, endpointB]
  const wrapper = mount(EndpointEditor, {
    global: { directives: { 'tooltip-overflow': {}, 'focus-end': {} } },
  })
  await nextTick()
  return { wrapper, store }
}

describe('EndpointEditor：分割条手动态不跨接口', () => {
  it('A 双击折叠分割条后打开 B：请求卡回到 grow 贴内容布局（无固定高内联样式）', async () => {
    const { wrapper, store } = await mountEditor()

    store.openEndpoint(endpointA)
    await nextTick()
    const box = () => wrapper.find('.config-box')
    expect(box().classes()).toContain('grow')

    // 双击分割条：进入手动分割并折叠请求卡（grow 撤销、写内联高度）
    await wrapper.find('.rp-splitter').trigger('dblclick')
    await nextTick()
    expect(box().classes()).not.toContain('grow')
    expect(box().attributes('style') ?? '').toContain('height')

    // 打开新接口（无响应）：不得残留上一接口的固定高
    store.openEndpoint(endpointB)
    await nextTick()
    expect(box().classes()).toContain('grow')
    expect(box().classes()).not.toContain('collapsed')
    expect(box().attributes('style') ?? '').not.toContain('height')

    wrapper.unmount()
  })

  it('A 拖拽分割条后打开 B：B 同样回到 grow，切回 A 也不残留固定高', async () => {
    const { wrapper, store } = await mountEditor()

    store.openEndpoint(endpointA)
    await nextTick()
    const box = () => wrapper.find('.config-box')

    // 拖拽分割条（mousedown 即进入手动分割模式）
    await wrapper.find('.rp-splitter').trigger('mousedown')
    document.dispatchEvent(new MouseEvent('mouseup'))
    await nextTick()
    expect(box().classes()).not.toContain('grow')

    store.openEndpoint(endpointB)
    await nextTick()
    expect(box().classes()).toContain('grow')

    store.openEndpoint(endpointA)
    await nextTick()
    expect(box().classes()).toContain('grow')

    wrapper.unmount()
  })
})
