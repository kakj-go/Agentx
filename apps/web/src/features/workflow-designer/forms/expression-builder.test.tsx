import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import type { ReferenceCatalog, ValueSelector } from '../model/types'
import { ExpressionBuilder } from './expression-builder'

describe('ExpressionBuilder', () => {
  it('renders a structured reference with its catalog label', () => {
    const selector: ValueSelector = { namespace: 'inputs', run: { kind: 'current' }, item: { kind: 'current' }, path: ['amount'] }
    const catalog: ReferenceCatalog = {
      inputs: [{ id: 'inputs.amount', label: 'amount', path: 'inputs.amount', selector, type: 'number', children: [] }],
      outputs: [],
      contexts: [],
    }
    render(<ExpressionBuilder allowed={['inputs']} catalog={catalog} onChange={vi.fn()} value={{ kind: 'reference', selector, missingPolicy: { kind: 'error' } }} />)

    expect(screen.getByRole('button', { name: /amount/ })).toBeInTheDocument()
    expect(screen.getByTestId('expression-builder')).toHaveTextContent('amount')
  })

  it('edits schemaless reference paths as selector segments', () => {
    const onChange = vi.fn()
    const selector: ValueSelector = { namespace: 'item', run: { kind: 'current' }, item: { kind: 'current' }, path: [] }
    render(<ExpressionBuilder allowed={['item']} onChange={onChange} value={{ kind: 'reference', selector, missingPolicy: { kind: 'error' } }} />)

    fireEvent.change(screen.getByRole('textbox', { name: 'Reference path' }), { target: { value: 'score.0' } })
    expect(onChange).toHaveBeenCalledWith({ kind: 'reference', selector: { ...selector, path: ['score', 0] }, missingPolicy: { kind: 'error' } })
  })
})
