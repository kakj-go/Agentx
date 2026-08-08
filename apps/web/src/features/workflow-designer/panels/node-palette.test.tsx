import * as Tooltip from '@radix-ui/react-tooltip'
import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import type { NodeManifest } from '../model/types'
import { NodePalette } from './node-palette'

const manifest = (nodeType: string, inputKind: 'main' | 'error', outputKind: 'main' | 'error' = 'main'): NodeManifest => ({
  protocolVersion: '1.0', nodeType, version: 1, displayName: nodeType, description: `${nodeType} description`, category: 'actions', keywords: [nodeType], iconKey: 'box', executionStyle: 'action', capability: 'builtin', readiness: 'any',
  inputPorts: [{ name: inputKind, kind: inputKind, required: false, variadic: false }], outputPorts: [{ name: outputKind, kind: outputKind, required: false, variadic: false }], bindingSlots: nodeType === 'main-target' ? [{ name: 'ai_model', resourceType: 'model', required: false, multiple: false }] : [],
  parameterSchema: {}, uiSchema: { canvas: { role: 'default' } }, providers: [], credentials: [], retryPolicy: { retryable: false, maxAttempts: 1, initialBackoffMs: 0, maxBackoffMs: 0 }, sandboxRequired: false, supportsMock: true, sideEffectLevel: 'none',
})

function renderPalette(props: Partial<React.ComponentProps<typeof NodePalette>> = {}) {
  const manifests = [manifest('source', 'main', 'error'), manifest('main-target', 'main'), manifest('error-target', 'error')]
  return render(<Tooltip.Provider><NodePalette manifests={manifests} onAddAction={vi.fn()} onAddBinding={vi.fn()} {...props} /></Tooltip.Provider>)
}

describe('NodePalette', () => {
  it('starts as a single circular add button and opens an auto-focused overlay creator', () => {
    renderPalette()
    expect(screen.queryByTestId('node-creator')).not.toBeInTheDocument()
    expect(screen.getByTestId('node-creator-trigger')).toHaveClass('rounded-full')
    expect(screen.queryByTestId('node-creator-rail')).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Search nodes' }))
    expect(screen.getByTestId('node-creator')).toBeInTheDocument()
    expect(screen.getByRole('textbox', { name: 'Search nodes' })).toHaveFocus()
  })

  it('collapses every group except the first one by default', () => {
    renderPalette({ open: true })
    expect(screen.getByTestId('palette-group-integrations')).toHaveAttribute('aria-expanded', 'true')
    expect(screen.getByTestId('palette-group-attachments')).toHaveAttribute('aria-expanded', 'false')
    expect(screen.getByTestId('palette-action-source')).toBeInTheDocument()
    expect(screen.queryByTestId('palette-binding-model')).not.toBeInTheDocument()
    fireEvent.click(screen.getByTestId('palette-group-attachments'))
    expect(screen.getByTestId('palette-binding-model')).toBeInTheDocument()
  })

  it('filters by the exact source handle kind and hides binding attachments', () => {
    const source = manifest('source', 'main', 'error')
    renderPalette({ open: true, sourceConnection: { manifest: source, handleId: 'error' } })

    expect(screen.getByTestId('palette-action-error-target')).toBeInTheDocument()
    expect(screen.queryByTestId('palette-action-main-target')).not.toBeInTheDocument()
    expect(screen.queryByTestId('palette-binding-model')).not.toBeInTheDocument()
  })

  it('supports arrow-key navigation through creator results', () => {
    renderPalette({ open: true })
    const search = screen.getByRole('textbox', { name: 'Search nodes' })
    fireEvent.keyDown(search, { key: 'ArrowDown' })
    expect(screen.getByTestId('palette-action-source')).toHaveFocus()
    fireEvent.keyDown(screen.getByTestId('palette-action-source'), { key: 'ArrowDown' })
    expect(screen.getByTestId('palette-action-main-target')).toHaveFocus()
  })

  it('searches names and descriptions from every manifest locale', () => {
    const localized = manifest('localized', 'main')
    localized.localizations = {
      'zh-CN': { displayName: '中文节点', description: '仅中文用途说明', keywords: ['中文关键词'] },
      'en-US': { displayName: 'English node', description: 'English-only purpose', keywords: ['english-keyword'] },
    }
    render(<Tooltip.Provider><NodePalette manifests={[localized]} onAddAction={vi.fn()} onAddBinding={vi.fn()} open /></Tooltip.Provider>)
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
    render(<Tooltip.Provider><NodePalette manifests={[target]} onAddAction={onAddAction} onAddBinding={vi.fn()} open sourceConnection={{ manifest: source, handleId: 'main' }} /></Tooltip.Provider>)

    fireEvent.click(screen.getByTestId('palette-action-multi-input'))
    expect(onAddAction).not.toHaveBeenCalled()
    expect(screen.getByTestId('node-port-choice')).toBeInTheDocument()
    fireEvent.click(screen.getByTestId('port-choice-right'))
    expect(onAddAction).toHaveBeenCalledWith(target, 'right')
  })
})
