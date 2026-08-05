import { fireEvent, render, screen } from '@testing-library/react'
import { I18nextProvider } from 'react-i18next'
import type { ComponentProps } from 'react'
import { describe, expect, it, vi } from 'vitest'

import { i18n } from '../../app/i18n'
import type { RuntimeDetails } from '../../shared/api/types'
import { ExecutionRuntimePanel } from './execution-runtime-panel'
import { formatRuntimeTimestamp } from './runtime-format'

const details: RuntimeDetails = {
  executionId: 'execution-1',
  inputTokens: 12,
  outputTokens: 8,
  costMicros: 42,
  agentRuns: [{
    id: 'run-1', nodeExecutionId: 'node-1', status: 'succeeded', stopReason: 'stop', iterationCount: 1,
    modelCallCount: 1, toolCallCount: 0, inputTokens: 12, outputTokens: 8, costMicros: 42,
    budget: {}, stateHash: 'a'.repeat(64), stateArtifactId: 'artifact-1', startedAt: '2026-08-04T00:00:00Z', endedAt: '2026-08-04T00:00:01Z',
  }],
  iterations: [{
    id: 'iteration-1', agentRunId: 'run-1', iterationIndex: 0, status: 'completed', stateBeforeHash: 'b'.repeat(64),
    stateAfterHash: 'a'.repeat(64), stateArtifactId: null, stopReason: 'stop', startedAt: '2026-08-04T00:00:00Z', endedAt: '2026-08-04T00:00:01Z',
  }],
  calls: [{
    id: 'call-1', attemptId: 'attempt-1', agentRunId: 'run-1', iterationIndex: 0, callIndex: 0,
    callKind: 'model', requestFingerprint: 'c'.repeat(64), resourceType: 'model', resourceId: 'model-1',
    resourceVersionId: 'model-version-1', sideEffect: 'none', status: 'succeeded', inputTokens: 12,
    outputTokens: 8, costMicros: 42, usageEstimated: false, responseArtifactId: 'artifact-1', errorCode: null,
    errorMessage: null, startedAt: '2026-08-04T00:00:00Z', endedAt: '2026-08-04T00:00:01Z',
  }],
  sandboxes: [{
    id: 'lease-1', nodeExecutionId: 'node-1', attemptId: 'attempt-1', profileVersionId: 'profile-version-1',
    sandboxId: 'sandbox-1', status: 'terminated', expiresAt: '2026-08-04T00:05:00Z', heartbeatAt: '2026-08-04T00:00:01Z',
    terminationAttempts: 0, lastError: null, createdAt: '2026-08-04T00:00:00Z', terminatedAt: '2026-08-04T00:00:01Z',
  }],
}

function renderPanel(props: Partial<ComponentProps<typeof ExecutionRuntimePanel>> = {}) {
  render(<I18nextProvider i18n={i18n}><ExecutionRuntimePanel details={props.details} error={props.error} loading={props.loading ?? false} onDownloadArtifact={props.onDownloadArtifact ?? vi.fn()} /></I18nextProvider>)
}

describe('ExecutionRuntimePanel', () => {
  it('renders loading, error, and empty states', async () => {
    await i18n.changeLanguage('zh-CN')
    const { rerender } = render(<I18nextProvider i18n={i18n}><ExecutionRuntimePanel loading onDownloadArtifact={vi.fn()} /></I18nextProvider>)
    expect(screen.getByText('正在加载 Agent Runtime 明细…')).toBeInTheDocument()
    rerender(<I18nextProvider i18n={i18n}><ExecutionRuntimePanel error={new Error('runtime denied')} loading={false} onDownloadArtifact={vi.fn()} /></I18nextProvider>)
    expect(screen.getByText('runtime denied')).toHaveClass('text-danger')
    rerender(<I18nextProvider i18n={i18n}><ExecutionRuntimePanel details={{ ...details, agentRuns: [], iterations: [], calls: [], sandboxes: [] }} loading={false} onDownloadArtifact={vi.fn()} /></I18nextProvider>)
    expect(screen.getByText('该 Execution 没有 Agent、Runtime Call 或 Sandbox 记录。')).toBeInTheDocument()
  })

  it('shows the agent, call, sandbox, budget, stop reason, and artifact controls', async () => {
    await i18n.changeLanguage('zh-CN')
    const onDownload = vi.fn()
    renderPanel({ details, onDownloadArtifact: onDownload })
    expect(screen.getByRole('heading', { name: 'Agent Runtime 明细' })).toBeInTheDocument()
    expect(screen.getAllByText('stop')).toHaveLength(2)
    expect(screen.getByRole('tab', { name: /Runtime Calls 1/ })).toBeInTheDocument()
    expect(screen.getByRole('tab', { name: /Sandboxes 1/ })).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Download artifact artifact-1' }))
    expect(onDownload).toHaveBeenCalledWith('artifact-1')
  })

  it('does not expose invalid sandbox timestamps', () => {
    expect(formatRuntimeTimestamp('not-a-timestamp')).toBe('—')
  })
})
