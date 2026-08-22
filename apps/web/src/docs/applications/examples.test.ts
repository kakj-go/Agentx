import { describe, expect, it } from 'vitest'

import { apiKeyExample, applicationInvocationUrl, integrationLanguages, schemaExample, webhookExample, webhookUrl } from './examples'

const inputSchema = {
  type: 'object',
  properties: {
    message: { type: 'string' },
    priority: { type: 'integer' },
    metadata: { type: 'object', properties: { urgent: { type: 'boolean' } } },
  },
}

describe('application integration document examples', () => {
  it('builds safe API key examples from the published input schema', () => {
    expect(schemaExample(inputSchema)).toEqual({ message: 'example', priority: 0, metadata: { urgent: true } })
    for (const language of integrationLanguages) {
      const code = apiKeyExample('createInvocation', language, 'https://runtime.agentx.test/', 'customer support', inputSchema)
      expect(code).toContain('https://runtime.agentx.test/gateway/v1/applications/customer%20support/invocations')
      expect(code).toContain('AGENTX_API_KEY')
      expect(code).not.toContain('axk_')
    }
  })

  it('builds Webhook HMAC signers in every documented language', () => {
    for (const language of integrationLanguages) {
      const code = webhookExample(language, 'https://runtime.agentx.test', '/gateway/v1/webhooks/public-id', inputSchema)
      expect(code).toContain('AGENTX_WEBHOOK_SECRET')
      expect(code).toContain('X-Agentx-Signature')
      expect(code).toContain('https://runtime.agentx.test/gateway/v1/webhooks/public-id')
    }
    expect(webhookUrl('https://runtime.agentx.test/', 'gateway/v1/webhooks/public-id')).toBe('https://runtime.agentx.test/gateway/v1/webhooks/public-id')
  })

  it('documents every API key endpoint with a runnable request shape', () => {
    for (const endpoint of ['createInvocation', 'createSession', 'sendMessage', 'getInvocation', 'streamEvents', 'cancelInvocation'] as const) {
      for (const language of integrationLanguages) {
        expect(apiKeyExample(endpoint, language, 'https://runtime.agentx.test', 'orders', inputSchema)).toContain('AGENTX_API_KEY')
      }
    }
    expect(apiKeyExample('getInvocation', 'curl', 'https://runtime.agentx.test', 'orders', inputSchema)).toContain('"https://runtime.agentx.test/gateway/v1/invocations/$INVOCATION_ID"')
    expect(apiKeyExample('getInvocation', 'curl', 'https://runtime.agentx.test', 'orders', inputSchema)).toContain('"Authorization: Bearer $AGENTX_API_KEY"')
    expect(apiKeyExample('createInvocation', 'java', 'https://runtime.agentx.test', 'orders', inputSchema)).toContain('public class AgentxExample')
    expect(apiKeyExample('createInvocation', 'node', 'https://runtime.agentx.test', 'orders', inputSchema)).toContain("'Bearer ' + process.env.AGENTX_API_KEY")
  })

  it('builds the canonical application invocation route', () => {
    expect(applicationInvocationUrl('https://runtime.agentx.test/', 'orders')).toBe('https://runtime.agentx.test/gateway/v1/applications/orders/invocations')
  })
})
