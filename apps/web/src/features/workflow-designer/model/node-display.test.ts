import { describe, expect, it } from 'vitest'

import { buildDefaultNameIndex, localizedExitLabel, resolveNodeDisplayName, resolveSpanDisplayName, type NodeDisplayTexts } from './node-display'
import { studioManifest, studioManifestTypes } from '../testing/studio-catalog'

const manifests = studioManifestTypes.map((nodeType) => studioManifest(nodeType))
const manifestsByType = new Map(manifests.map((manifest) => [manifest.nodeType, manifest]))

function textsFor(language: string, overrides: Partial<NodeDisplayTexts> = {}): NodeDisplayTexts {
  return {
    language,
    exitDefaultName: language === 'zh-CN' ? '结束' : 'End',
    spanKindLabel: (kind) => (language === 'zh-CN'
      ? { attempt: '尝试', agent_iteration: '智能体迭代', agent_run: '智能体运行', runtime_call: 'Runtime 调用', sandbox: '沙箱' }[kind] ?? kind
      : kind),
    waitApprovalLabel: (title) => (language === 'zh-CN' ? `审批 · ${title}` : `Approval · ${title}`),
    waitLabel: (title) => (language === 'zh-CN' ? `等待 · ${title}` : `Wait · ${title}`),
    ...overrides,
  }
}

const defaultsByName = buildDefaultNameIndex(manifests)

describe('resolveNodeDisplayName', () => {
  it('maps stored English defaults to the requested language', () => {
    expect(resolveNodeDisplayName('Loop Over Items', 'loop_over_items', manifestsByType, defaultsByName, textsFor('zh-CN'))).toBe('循环处理')
    expect(resolveNodeDisplayName('Condition', 'if', manifestsByType, defaultsByName, textsFor('zh-CN'))).toBe('条件分支')
    expect(resolveNodeDisplayName('Loop Over Items', 'loop_over_items', manifestsByType, defaultsByName, textsFor('en-US'))).toBe('Loop Over Items')
  })

  it('resolves names without a node type via the default-name index', () => {
    expect(resolveNodeDisplayName('Agent', undefined, manifestsByType, defaultsByName, textsFor('zh-CN'))).toBe('智能体')
  })

  it('passes user-renamed nodes through unchanged', () => {
    expect(resolveNodeDisplayName('检查输入', 'if', manifestsByType, defaultsByName, textsFor('zh-CN'))).toBe('检查输入')
    expect(resolveNodeDisplayName('My custom step', 'model', manifestsByType, defaultsByName, textsFor('en-US'))).toBe('My custom step')
  })

  it('localizes default exit names with their suffix', () => {
    expect(resolveNodeDisplayName('End', 'exit', manifestsByType, defaultsByName, textsFor('zh-CN'))).toBe('结束')
    expect(resolveNodeDisplayName('End 2', 'exit', manifestsByType, defaultsByName, textsFor('zh-CN'))).toBe('结束 2')
    expect(resolveNodeDisplayName('Endpoint', 'exit', manifestsByType, defaultsByName, textsFor('zh-CN'))).toBe('Endpoint')
  })
})

describe('resolveSpanDisplayName', () => {
  it('localizes backend-generated span labels', () => {
    const texts = textsFor('zh-CN')
    expect(resolveSpanDisplayName('Attempt 2', manifestsByType, defaultsByName, texts)).toBe('尝试 2')
    expect(resolveSpanDisplayName('Iteration 3', manifestsByType, defaultsByName, texts)).toBe('智能体迭代 3')
    expect(resolveSpanDisplayName('Agent run', manifestsByType, defaultsByName, texts)).toBe('智能体运行')
    expect(resolveSpanDisplayName('Runtime call', manifestsByType, defaultsByName, texts)).toBe('Runtime 调用')
    expect(resolveSpanDisplayName('OpenSandbox execution', manifestsByType, defaultsByName, texts)).toBe('沙箱')
    expect(resolveSpanDisplayName('Approval · 请审批', manifestsByType, defaultsByName, texts)).toBe('审批 · 请审批')
  })

  it('localizes node span names and keeps custom names', () => {
    const texts = textsFor('zh-CN')
    expect(resolveSpanDisplayName('Condition', manifestsByType, defaultsByName, texts)).toBe('条件分支')
    expect(resolveSpanDisplayName('My custom step', manifestsByType, defaultsByName, texts)).toBe('My custom step')
  })
})

describe('localizedExitLabel', () => {
  it('keeps renamed exits untouched', () => {
    expect(localizedExitLabel('最终出口', '结束')).toBe('最终出口')
    expect(localizedExitLabel('Endgame', 'End')).toBe('Endgame')
  })
})
