import { act, render, screen, waitFor } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import { $getSelection, $getRoot, $isNodeSelection, $isRangeSelection, KEY_ARROW_LEFT_COMMAND, KEY_ARROW_RIGHT_COMMAND, type LexicalEditor } from 'lexical'

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

  it('moves the caret across a standalone reference chip with arrow keys', async () => {
    let editor: LexicalEditor | undefined
    render(<VariableTokenEditor onChange={vi.fn()} onEditorReady={(value) => { editor = value }} value={{ kind: 'reference', selector, missingPolicy: { kind: 'error' } }} />)
    await waitFor(() => expect(editor).toBeDefined())

    act(() => editor!.update(() => {
      $getRoot().getFirstChildOrThrow().selectEnd()
      editor!.dispatchCommand(KEY_ARROW_LEFT_COMMAND, new KeyboardEvent('keydown', { key: 'ArrowLeft' }))
    }, { discrete: true }))
    expect(readCaret(editor!)).toEqual({ type: 'element', offset: 0 })

    act(() => editor!.update(() => {
      editor!.dispatchCommand(KEY_ARROW_RIGHT_COMMAND, new KeyboardEvent('keydown', { key: 'ArrowRight' }))
    }, { discrete: true }))
    expect(readCaret(editor!)).toEqual({ type: 'element', offset: 1 })
  })
})

function readCaret(editor: LexicalEditor) {
  let caret: { type: string; offset: number } | undefined
  editor.getEditorState().read(() => {
    const selection = $getSelection()
    if ($isRangeSelection(selection)) caret = { type: selection.anchor.type, offset: selection.anchor.offset }
    else if ($isNodeSelection(selection)) caret = { type: 'node', offset: selection.getNodes().length }
  })
  return caret
}
