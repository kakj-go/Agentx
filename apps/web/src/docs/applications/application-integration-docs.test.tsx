import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { ApplicationIntegrationDocs } from './application-integration-docs'

const deployment = {
  inputSchema: { type: 'object', properties: { message: { type: 'string', description: 'User message' } }, required: ['message'] },
  outputSchema: { type: 'object', properties: { answer: { type: 'string', description: 'Agent answer' } } },
}

function selectTab(name: RegExp | string) {
  const tab = screen.getByRole('tab', { name })
  fireEvent.mouseDown(tab, { button: 0, ctrlKey: false })
  fireEvent.click(tab)
}

describe('ApplicationIntegrationDocs', () => {
  it('renders an application-specific API key guide and schemas', () => {
    render(<ApplicationIntegrationDocs activeDeployment={deployment} applicationSlug="support-agent" document={{ kind: 'apiKey' }} onOpenChange={vi.fn()} open runtimeBaseUrl="https://runtime.agentx.test" />)

    expect(screen.getByRole('dialog', { name: /API Key 接入文档|API key integration guide/ })).toBeVisible()
    expect(screen.getByText('https://runtime.agentx.test/gateway/v1/applications/support-agent/invocations')).toBeVisible()
    expect(screen.getAllByText(/Bearer \$AGENTX_API_KEY/).length).toBeGreaterThan(0)

    expect(screen.getByText(/"message": "example"/)).toBeVisible()
    expect(screen.getByRole('tab', { name: /数据结构|Schemas/ })).toBeVisible()

    selectTab(/接口说明|API reference/)
    expect(screen.getAllByText(/创建无会话调用|Create a stateless invocation/).length).toBeGreaterThan(0)
    expect(screen.getByText('responseMode')).toBeVisible()
    expect(screen.getByRole('tab', { name: 'Java' })).toBeVisible()
    expect(screen.getByRole('tab', { name: 'Go' })).toBeVisible()
    expect(screen.getByRole('tab', { name: 'Node.js' })).toBeVisible()
    expect(screen.getByRole('tab', { name: 'Python' })).toBeVisible()
  })

  it('renders the selected channel URL and provider contract', () => {
    render(<ApplicationIntegrationDocs activeDeployment={deployment} applicationSlug="support-agent" document={{ kind: 'webhook', name: 'CRM Hook', path: '/gateway/v1/webhooks/public-id' }} onOpenChange={vi.fn()} open runtimeBaseUrl="https://runtime.agentx.test" />)

    expect(screen.getByText('https://runtime.agentx.test/gateway/v1/webhooks/public-id')).toBeVisible()
    selectTab(/安全|Security/)
    expect(screen.getByText(/Verification and decryption|验证与解密/)).toBeVisible()
    expect(screen.getByText(/DingTalk: timestamp|钉钉：timestamp/)).toBeVisible()
  })

  it('shows deployed schemas without a copyable endpoint before a channel is active', () => {
    render(<ApplicationIntegrationDocs activeDeployment={deployment} applicationSlug="support-agent" document={{ kind: 'webhook' }} onOpenChange={vi.fn()} open runtimeBaseUrl="https://runtime.agentx.test" />)

    expect(screen.getAllByText(/The production endpoint activates after the channel is saved and published|生产接入地址将在渠道保存并发布后激活/).length).toBeGreaterThan(0)
    expect(screen.getAllByText(/A channel only accepts inbound events|渠道只负责接收入站事件/).length).toBeGreaterThan(0)
    selectTab(/数据结构|Schemas/)
    expect(screen.getByText('message')).toBeVisible()
    expect(screen.getByText(/数据结构来自当前应用的已激活部署|Schemas come from the current active application deployment/)).toBeVisible()
  })
})
