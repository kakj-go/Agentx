import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { Dialog, DialogContent } from '../../../../shared/ui/dialog'
import type { ReferenceCatalog } from '../../model/types'
import { ReferencePicker, schemaCompatible } from './reference-picker'

const catalog: ReferenceCatalog = {
  inputs: [{ id: 'inputs.question', label: 'question', path: 'inputs.question', selector: { namespace: 'inputs', run: { kind: 'current' }, item: { kind: 'current' }, path: ['question'] }, type: 'string', schema: { type: 'string' }, children: [] }],
  outputs: [],
  contexts: [],
}

describe('ReferencePicker', () => {
  it('checks array items and required object fields with full schemas', () => {
    expect(schemaCompatible({ type: 'integer' }, { type: 'number' })).toBe(true)
    expect(schemaCompatible({ type: 'array', items: { type: 'string' } }, { type: 'array', items: { type: 'number' } })).toBe(false)
    expect(schemaCompatible(
      { type: 'object', properties: { name: { type: 'string' } } },
      { type: 'object', required: ['id'], properties: { id: { type: 'integer' } } },
    )).toBe(false)
  })
  it('shows execution information only when the field contract allows it', () => {
    const executionLabel = ['Execution', 'information'].join(' ')
    const executionCatalog: ReferenceCatalog = {
      ...catalog,
      execution: [{ id: 'execution.root', label: executionLabel, path: 'execution', children: [] }],
    }
    const rendered = render(<ReferencePicker catalog={executionCatalog} onInsert={vi.fn()} onOpenChange={vi.fn()} open />)
    expect(screen.queryByText(executionLabel)).not.toBeInTheDocument()

    rendered.rerender(<ReferencePicker allowedNamespaces={['execution']} catalog={executionCatalog} onInsert={vi.fn()} onOpenChange={vi.fn()} open />)
    expect(screen.getByText(executionLabel)).toBeInTheDocument()
  })

  it('inserts a selected reference and closes', () => {
    const insert = vi.fn()
    const open = vi.fn()
    render(<ReferencePicker catalog={catalog} onInsert={insert} onOpenChange={open} open />)

    fireEvent.click(screen.getByText('Inputs'))
    expect(screen.getByText('string')).toBeInTheDocument()
    expect(screen.queryByText('Details')).not.toBeInTheDocument()
    fireEvent.click(screen.getByText('question'))

    expect(insert).toHaveBeenCalledWith(catalog.inputs[0].selector, catalog.inputs[0])
    expect(open).toHaveBeenCalledWith(false)
  })

  it('groups output references under source node headers with type color dots', () => {
    const insert = vi.fn()
    const open = vi.fn()
    const treeCatalog: ReferenceCatalog = {
      ...catalog,
      outputs: [{ id: 'outputs.model', label: 'model', path: 'outputs.model', sourceNodeType: 'model', children: [{ id: 'outputs.model.main', label: 'main', path: 'outputs.model.main', children: [{ id: 'outputs.model.main.first', label: 'first', path: 'outputs.model.main.first', children: [{ id: 'outputs.model.main.first.json.text', label: 'text', path: 'outputs.model.main.first.json.text', selector: { namespace: 'outputs', sourceNodeId: 'model', port: 'main', run: { kind: 'current' }, item: { kind: 'first' }, path: ['text'] }, type: 'string', recommended: true, children: [] }] }] }] }],
    }
    render(<ReferencePicker catalog={treeCatalog} onInsert={insert} onOpenChange={open} open />)

    fireEvent.click(screen.getByText('Outputs'))
    const group = screen.getByTestId('reference-group-model')
    expect(group).toHaveTextContent('model')
    const dot = group.querySelector<HTMLElement>('span[style]')
    expect(dot).toHaveStyle({ backgroundColor: '#6366f1' })
    const main = screen.getByRole('button', { name: /main/ })
    expect(main).toHaveAttribute('aria-expanded', 'false')
    fireEvent.click(main)
    expect(main).toHaveAttribute('aria-expanded', 'true')
    fireEvent.click(screen.getByRole('button', { name: /first/ }))
    expect(screen.getByText(/Recommended|推荐/)).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: /text/ }))

    expect(screen.queryByRole('button', { name: /Back|返回/ })).not.toBeInTheDocument()
    const field = treeCatalog.outputs[0].children[0].children[0].children[0]
    expect(insert).toHaveBeenCalledWith(field.selector, field)
    expect(open).toHaveBeenCalledWith(false)
  })

  it('closes on Escape and outside click', () => {
    const escape = vi.fn()
    const rendered = render(<ReferencePicker catalog={catalog} onInsert={vi.fn()} onOpenChange={escape} open />)
    fireEvent.keyDown(document, { key: 'Escape' })
    expect(escape).toHaveBeenCalledWith(false)

    rendered.rerender(<ReferencePicker catalog={catalog} onInsert={vi.fn()} onOpenChange={escape} open />)
    fireEvent.mouseDown(document.body)
    expect(escape).toHaveBeenCalledTimes(2)
  })

  it('renders inside the interactive layer of a modal dialog', () => {
    render(<Dialog open><DialogContent title="Output field"><ReferencePicker catalog={catalog} onInsert={vi.fn()} onOpenChange={vi.fn()} open /></DialogContent></Dialog>)

    const picker = screen.getByTestId('reference-picker')
    const dialog = screen.getByRole('dialog')
    expect(picker).toHaveClass('absolute', 'pointer-events-auto', 'z-[100]')
    expect(dialog).toHaveClass('z-[210]')
    expect(dialog.previousElementSibling).toHaveClass('z-[200]')
    expect(picker.closest('[role="dialog"]')).toBe(dialog)
  })

  it('anchors below a right-side input instead of beside its container', () => {
    const anchor = document.createElement('div')
    const rect = { bottom: 236, height: 36, left: 820, right: 960, top: 200, width: 140, x: 820, y: 200, toJSON: () => ({}) }
    vi.spyOn(anchor, 'getBoundingClientRect').mockReturnValue(rect as DOMRect)

    render(<ReferencePicker anchorRef={{ current: anchor }} catalog={catalog} onInsert={vi.fn()} onOpenChange={vi.fn()} open />)

    expect(screen.getByTestId('reference-picker')).toHaveStyle({ height: '300px', left: '640px', top: '242px', width: '320px' })
  })

  it('flips above the input when there is not enough room below', () => {
    const anchor = document.createElement('div')
    const rect = { bottom: 736, height: 36, left: 400, right: 820, top: 700, width: 420, x: 400, y: 700, toJSON: () => ({}) }
    vi.spyOn(anchor, 'getBoundingClientRect').mockReturnValue(rect as DOMRect)

    render(<ReferencePicker anchorRef={{ current: anchor }} catalog={catalog} onInsert={vi.fn()} onOpenChange={vi.fn()} open />)

    expect(screen.getByTestId('reference-picker')).toHaveStyle({ height: '300px', left: '400px', top: '394px', width: '420px' })
  })

  it('allows unknown values for runtime conversion to typed targets', () => {
    const insert = vi.fn()
    const unknownCatalog: ReferenceCatalog = { ...catalog, inputs: [{ ...catalog.inputs[0], type: undefined, schema: undefined }] }
    render(<ReferencePicker catalog={unknownCatalog} expectedSchema={{ type: 'string' }} onInsert={insert} onOpenChange={vi.fn()} open />)
    fireEvent.click(screen.getByText('Inputs'))
    const field = screen.getByRole('button', { name: /question/ })
    expect(field).not.toBeDisabled()
    fireEvent.click(field)
    expect(insert).toHaveBeenCalled()
  })

  it('keeps an incompatible object branch enabled so its compatible children remain reachable', () => {
    const insert = vi.fn()
    const selector = { namespace: 'contexts' as const, run: { kind: 'current' as const }, item: { kind: 'current' as const }, path: ['profile'] }
    const objectCatalog: ReferenceCatalog = {
      ...catalog,
      contexts: [{ id: 'contexts.profile', label: 'profile', path: 'contexts.profile', selector, type: 'object', schema: { type: 'object', properties: { name: { type: 'string' } } }, children: [{ id: 'contexts.profile.name', label: 'name', path: 'contexts.profile.name', selector: { ...selector, path: ['profile', 'name'] }, type: 'string', schema: { type: 'string' }, children: [] }] }],
    }
    render(<ReferencePicker catalog={objectCatalog} expectedSchema={{ type: 'string' }} onInsert={insert} onOpenChange={vi.fn()} open />)
    fireEvent.click(screen.getByRole('button', { name: /Global variables|Contexts|全局变量/ }))
    const object = screen.getByRole('button', { name: /profile/ })
    fireEvent.click(object.querySelector('[data-tree-toggle]')!)
    expect(object).toHaveAttribute('aria-expanded', 'true')
    expect(screen.getByText('name')).toBeInTheDocument()
    expect(object).not.toHaveAttribute('aria-disabled')
    fireEvent.click(screen.getByText('name'))
    expect(insert).toHaveBeenCalledWith(expect.objectContaining({ path: ['profile', 'name'] }), expect.objectContaining({ type: 'string' }))
  })

  it('allows unknown values for runtime conversion to non-string targets', () => {
    const unknownCatalog: ReferenceCatalog = { ...catalog, inputs: [{ ...catalog.inputs[0], type: undefined, schema: undefined }] }
    render(<ReferencePicker catalog={unknownCatalog} expectedSchema={{ type: 'integer' }} onInsert={vi.fn()} onOpenChange={vi.fn()} open />)
    fireEvent.click(screen.getByText('Inputs'))
    expect(screen.getByRole('button', { name: /question/ })).not.toBeDisabled()
  })
})
