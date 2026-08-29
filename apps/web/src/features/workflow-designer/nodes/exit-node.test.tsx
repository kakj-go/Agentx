import { ReactFlowProvider, type NodeProps } from '@xyflow/react'
import { render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it } from 'vitest'

import { i18n } from '../../../app/i18n'
import type { CanvasNode, ExitNodeData } from '../model/types'
import { ExitNode } from './exit-node'

function props(data: ExitNodeData): NodeProps<CanvasNode> {
  return {
    id: 'exit-1',
    type: 'exit',
    data,
    selected: false,
  } as unknown as NodeProps<CanvasNode>
}

function renderExit(data: ExitNodeData) {
  return render(<ReactFlowProvider><ExitNode {...props(data)} /></ReactFlowProvider>)
}

afterEach(async () => {
  await i18n.changeLanguage('zh-CN')
})

describe('ExitNode', () => {
  it('renders main and error target ports with Chinese labels', async () => {
    await i18n.changeLanguage('zh-CN')
    renderExit({ editorKind: 'exit', key: 'exit', label: 'End', protected: false, parameters: { outputs: {}, errorOutputs: {} } })
    expect(screen.getByTestId('exit-node-exit')).toBeInTheDocument()
    expect(screen.getByTestId('exit-port-main')).toBeInTheDocument()
    expect(screen.getByTestId('exit-port-error')).toBeInTheDocument()
    expect(screen.getByTestId('exit-port-label-main')).toHaveTextContent('输出')
    expect(screen.getByTestId('exit-port-label-error')).toHaveTextContent('错误')
    expect(screen.queryByTestId('exit-protected')).not.toBeInTheDocument()
  })

  it('shows the lock marker on the protected exit', async () => {
    await i18n.changeLanguage('zh-CN')
    renderExit({ editorKind: 'exit', key: 'exit', label: 'End', protected: true, parameters: { outputs: {}, errorOutputs: {} } })
    expect(screen.getByTestId('exit-protected')).toBeInTheDocument()
  })
})
