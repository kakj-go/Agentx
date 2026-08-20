import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { Dialog, DialogContent } from '../../../../shared/ui/dialog'
import type { ReferenceCatalog } from '../../model/types'
import { ReferencePicker } from './reference-picker'

const catalog: ReferenceCatalog = {
  inputs: [{ id: 'inputs.question', label: 'question', path: 'inputs.question', selector: { namespace: 'inputs', run: { kind: 'current' }, item: { kind: 'current' }, path: ['question'] }, type: 'string', children: [] }],
  outputs: [],
  contexts: [],
}

describe('ReferencePicker', () => {
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

  it('expands output branches inline as a tree', () => {
    const insert = vi.fn()
    const open = vi.fn()
    const treeCatalog: ReferenceCatalog = {
      ...catalog,
      outputs: [{ id: 'outputs.model', label: 'model', path: 'outputs.model', children: [{ id: 'outputs.model.main', label: 'main', path: 'outputs.model.main', children: [{ id: 'outputs.model.main.first', label: 'first', path: 'outputs.model.main.first', children: [{ id: 'outputs.model.main.first.json.text', label: 'text', path: 'outputs.model.main.first.json.text', selector: { namespace: 'outputs', sourceNodeId: 'model', port: 'main', run: { kind: 'current' }, item: { kind: 'first' }, path: ['text'] }, type: 'string', children: [] }] }] }] }],
    }
    render(<ReferencePicker catalog={treeCatalog} onInsert={insert} onOpenChange={open} open />)

    fireEvent.click(screen.getByText('Outputs'))
    const model = screen.getByRole('button', { name: /model/ })
    expect(model).toHaveAttribute('aria-expanded', 'false')
    fireEvent.click(model)
    expect(model).toHaveAttribute('aria-expanded', 'true')
    fireEvent.click(screen.getByRole('button', { name: /main/ }))
    fireEvent.click(screen.getByRole('button', { name: /first/ }))
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
})
