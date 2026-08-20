import { render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import type { ReferenceCatalog, ValueSelector } from '../model/types'
import { selectorDisplayLabel, VariableTokenEditor } from './variable-token-editor'

const selector: ValueSelector = {
  namespace: 'outputs',
  sourceNodeId: 'model-node-id',
  port: 'main',
  run: { kind: 'current' },
  item: { kind: 'current' },
  path: ['text'],
}

const catalog: ReferenceCatalog = {
  inputs: [],
  contexts: [],
  outputs: [{
    id: 'outputs.model', label: 'Summarizer', path: 'outputs.model', children: [{
      id: 'outputs.model.main', label: 'main', path: 'outputs.model.main', children: [{
        id: 'outputs.model.main.current', label: 'current', path: 'outputs.model.main.current', children: [{
          id: 'outputs.model.main.current.text', label: 'text', path: 'outputs.model.main.current.text', selector, type: 'string', children: [],
        }],
      }],
    }],
  }],
}

describe('VariableTokenEditor', () => {
  it('derives a readable chip label without persisting display metadata', () => {
    expect(selectorDisplayLabel(selector, catalog)).toBe('Summarizer / main / text')
    render(<VariableTokenEditor catalog={catalog} onChange={vi.fn()} value={{ kind: 'reference', selector, missingPolicy: { kind: 'error' } }} />)

    expect(screen.getByText('Summarizer / main / text')).toBeInTheDocument()
  })

  it('renders literal text without placeholder syntax', () => {
    render(<VariableTokenEditor onChange={vi.fn()} value={{ kind: 'literal', value: 'plain text' }} />)

    expect(screen.getByRole('textbox', { name: 'Value' })).toHaveTextContent('plain text')
  })
})
