import { ReactFlowProvider, type NodeProps } from '@xyflow/react'
import { render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it } from 'vitest'

import { i18n } from '../../../app/i18n'
import type { CanvasNode } from '../model/types'
import { BoundaryNode } from './boundary-node'

function props(boundary: 'start' | 'end'): NodeProps<CanvasNode> {
  return {
    id: `__${boundary}__`,
    type: 'boundary',
    data: { editorKind: 'boundary', boundary, label: '' },
    selected: false,
  } as unknown as NodeProps<CanvasNode>
}

function renderBoundary(boundary: 'start' | 'end') {
  return render(<ReactFlowProvider><BoundaryNode {...props(boundary)} /></ReactFlowProvider>)
}

afterEach(async () => {
  await i18n.changeLanguage('zh-CN')
})

describe('BoundaryNode', () => {
  it('labels the Start input port in Chinese', async () => {
    await i18n.changeLanguage('zh-CN')
    renderBoundary('start')
    expect(screen.getByText('开始')).toBeInTheDocument()
    expect(screen.getByTestId('boundary-port-label-main')).toHaveTextContent('输入')
    expect(screen.getByTestId('boundary-port-main')).toHaveStyle({ top: '50%' })
  })

  it('separates and labels the End output and error ports', async () => {
    await i18n.changeLanguage('en-US')
    renderBoundary('end')
    const output = screen.getByTestId('boundary-port-label-main')
    const error = screen.getByTestId('boundary-port-label-error')
    expect(screen.getByText('End')).toBeInTheDocument()
    expect(output).toHaveTextContent('Output')
    expect(error).toHaveTextContent('Error')
    expect(output.style.top).toBe('30%')
    expect(error.style.top).toBe('70%')
    expect(screen.getByTestId('boundary-port-main').style.top).toBe(output.style.top)
    expect(screen.getByTestId('boundary-port-error').style.top).toBe(error.style.top)
  })
})
