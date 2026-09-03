import * as Tooltip from '@radix-ui/react-tooltip'
import { fireEvent, render, screen, within } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import type { NodeManifest } from '../model/types'
import { NodePalette } from './node-palette'

const manifest = (nodeType: string, inputKind: 'main' | 'error', outputKind: 'main' | 'error' = 'main'): NodeManifest => ({
  protocolVersion: '2.0', nodeType, version: 1, displayName: nodeType, description: `${nodeType} description`, category: 'actions', keywords: [nodeType], iconKey: 'box', executionStyle: 'action', capability: 'builtin', readiness: 'any',
  inputPorts: [{ name: inputKind, kind: inputKind, required: false, variadic: false }], outputPorts: [{ name: outputKind, kind: outputKind, required: false, variadic: false }], bindingSlots: [],
  parameterSchema: {}, uiSchema: { canvas: { role: 'default' } }, providers: [], credentials: [], retryPolicy: { retryable: false, maxAttempts: 1, initialBackoffMs: 0, maxBackoffMs: 0 }, sandboxRequired: false, supportsMock: true, sideEffectLevel: 'none',
})

function renderPalette(props: Partial<React.ComponentProps<typeof NodePalette>> = {}) {
  const manifests = [
    manifest('agent', 'main'),
    manifest('merge', 'main'),
    manifest('code', 'main'),
    manifest('declarative_http', 'main'),
    manifest('source', 'main', 'error'),
    manifest('error-target', 'error'),
  ]
  return render(<Tooltip.Provider><NodePalette manifests={manifests} onAddAction={vi.fn()} onAddExit={vi.fn()} onAddAnnotation={vi.fn()} onAddGroup={vi.fn()} {...props} /></Tooltip.Provider>)
}

describe('NodePalette', () => {
  it('renders as a persistent sidebar that groups nodes into the six visual groups', () => {
    renderPalette()
    expect(screen.getByTestId('node-creator')).toBeInTheDocument()
    expect(screen.queryByTestId('node-creator-trigger')).not.toBeInTheDocument()
    expect(screen.getByTestId('palette-group-ai')).toHaveAttribute('aria-expanded', 'true')
    expect(screen.getByTestId('palette-group-ai')).toHaveTextContent('AI')
    expect(screen.getByTestId('palette-group-logic')).toHaveAttribute('aria-expanded', 'false')
    expect(screen.getByTestId('palette-group-transform')).toHaveAttribute('aria-expanded', 'false')
    expect(screen.getByTestId('palette-group-integrate')).toHaveAttribute('aria-expanded', 'false')
    expect(screen.queryByTestId('palette-group-start')).not.toBeInTheDocument()
    expect(screen.queryByTestId('palette-group-output')).not.toBeInTheDocument()
    fireEvent.click(screen.getByTestId('palette-group-logic'))
    expect(screen.getByTestId('palette-action-merge')).toBeInTheDocument()
  })

  it('keeps the exit, note, and group actions in the tool row', () => {
    renderPalette()
    expect(screen.getByTestId('palette-exit')).toBeInTheDocument()
    const panel = within(screen.getByTestId('node-creator'))
    expect(panel.getByRole('button', { name: 'Sticky note' })).toBeInTheDocument()
    expect(panel.getByRole('button', { name: 'Group' })).toBeInTheDocument()
  })

  it('collapses into an icon rail that expands back into the palette', () => {
    renderPalette()
    fireEvent.click(screen.getByRole('button', { name: 'Collapse node panel' }))
    expect(screen.queryByTestId('node-creator')).not.toBeInTheDocument()
    expect(screen.getByTestId('palette-exit')).toBeInTheDocument()
    const trigger = screen.getByTestId('node-creator-trigger')
    expect(trigger).toHaveClass('rounded-full')
    fireEvent.click(trigger)
    expect(screen.getByTestId('node-creator')).toBeInTheDocument()
    expect(screen.getByRole('textbox', { name: 'Search nodes' })).toBeInTheDocument()
  })

  it('filters by the exact source handle kind when triggered from a port', () => {
    const source = manifest('source', 'main', 'error')
    const onAddExit = vi.fn()
    renderPalette({ sourceConnection: { manifest: source, handleId: 'error' }, onAddExit })

    expect(screen.getByTestId('palette-action-error-target')).toBeInTheDocument()
    expect(screen.queryByTestId('palette-action-agent')).not.toBeInTheDocument()
    fireEvent.click(screen.getByTestId('palette-exit'))
    expect(onAddExit).toHaveBeenCalledOnce()
  })

  it('resolves a dynamic branch handle through its manifest port and offers Exit for main flow', () => {
    const source = manifest('approval', 'main')
    source.outputPorts = [{ name: 'decision', kind: 'main', required: false, variadic: true }]
    const onAddExit = vi.fn()
    renderPalette({ sourceConnection: { manifest: source, handleId: 'decision:escalate', manifestPortName: 'decision' }, onAddExit })

    expect(screen.getByTestId('palette-action-agent')).toBeInTheDocument()
    fireEvent.click(screen.getByTestId('palette-exit'))
    expect(onAddExit).toHaveBeenCalledOnce()
  })

  it('supports arrow-key navigation through creator results', () => {
    renderPalette()
    fireEvent.click(screen.getByTestId('palette-group-logic'))
    const search = screen.getByRole('textbox', { name: 'Search nodes' })
    fireEvent.keyDown(search, { key: 'ArrowDown' })
    expect(screen.getByTestId('palette-action-agent')).toHaveFocus()
    fireEvent.keyDown(screen.getByTestId('palette-action-agent'), { key: 'ArrowDown' })
    expect(screen.getByTestId('palette-action-merge')).toHaveFocus()
  })

  it('searches names and descriptions from every manifest locale', () => {
    const localized = manifest('localized', 'main')
    localized.localizations = {
      'zh-CN': { displayName: '中文节点', description: '仅中文用途说明', keywords: ['中文关键词'] },
      'en-US': { displayName: 'English node', description: 'English-only purpose', keywords: ['english-keyword'] },
    }
    render(<Tooltip.Provider><NodePalette manifests={[localized]} onAddAction={vi.fn()} /></Tooltip.Provider>)
    const search = screen.getByRole('textbox', { name: 'Search nodes' })
    fireEvent.change(search, { target: { value: '仅中文用途说明' } })
    expect(screen.getByTestId('palette-action-localized')).toBeInTheDocument()
    fireEvent.change(search, { target: { value: 'English-only purpose' } })
    expect(screen.getByTestId('palette-action-localized')).toBeInTheDocument()
  })

  it('requires an explicit target handle when several inputs are compatible', () => {
    const source = manifest('source', 'main')
    const target = manifest('multi-input', 'main')
    target.inputPorts = [
      { name: 'left', kind: 'main', required: true, variadic: false },
      { name: 'right', kind: 'main', required: true, variadic: false },
    ]
    const onAddAction = vi.fn()
    render(<Tooltip.Provider><NodePalette manifests={[target]} onAddAction={onAddAction} sourceConnection={{ manifest: source, handleId: 'main' }} /></Tooltip.Provider>)

    fireEvent.click(screen.getByTestId('palette-action-multi-input'))
    expect(onAddAction).not.toHaveBeenCalled()
    expect(screen.getByTestId('node-port-choice')).toBeInTheDocument()
    fireEvent.click(screen.getByTestId('port-choice-right'))
    expect(onAddAction).toHaveBeenCalledWith(target, 'right')
  })

  it('drags palette items with the studio drop payload', () => {
    renderPalette()
    const item = screen.getByTestId('palette-action-agent')
    expect(item).toHaveAttribute('draggable', 'true')
  })
})
