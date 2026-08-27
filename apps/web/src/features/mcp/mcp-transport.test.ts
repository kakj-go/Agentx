import { describe, expect, it } from 'vitest'

import { buildMcpTransport, transportEndpoint } from './mcp-transport'

describe('MCP tagged transport', () => {
  it('builds HTTP and SSE without Runtime Sandbox fields', () => {
    expect(buildMcpTransport({
      transport: 'streamable_http', endpoint: 'https://mcp.example/mcp', credential: 'credential-1',
    })).toEqual({
      kind: 'streamable_http', endpoint: 'https://mcp.example/mcp', bearerCredentialId: 'credential-1',
    })
    expect(buildMcpTransport({ transport: 'sse', endpoint: 'https://mcp.example/sse' })).toEqual({
      kind: 'sse', endpoint: 'https://mcp.example/sse', bearerCredentialId: null,
    })
  })

  it('requires an exact Runtime Sandbox for stdio and freezes argv', () => {
    const transport = buildMcpTransport({
      transport: 'stdio',
      command: '/usr/local/bin/mcp-server',
      args: '["--stdio"]',
      environmentCredentials: '[{"name":"API_TOKEN","credentialId":"credential-1"}]',
      runtimeSandbox: 'sandbox-1:version-1',
    })
    expect(transport).toEqual({
      kind: 'stdio',
      command: '/usr/local/bin/mcp-server',
      args: ['--stdio'],
      environmentCredentialRefs: [{ name: 'API_TOKEN', credentialId: 'credential-1' }],
      runtimeSandbox: { resourceId: 'sandbox-1', resourceVersionId: 'version-1' },
    })
    expect(transportEndpoint(transport)).toBe('/usr/local/bin/mcp-server')
    expect(() => buildMcpTransport({ transport: 'stdio', command: '/bin/mcp', args: '[]', environmentCredentials: '[]' })).toThrow(/Sandbox/)
  })

  it('rejects shell strings and literal environment secrets in structured fields', () => {
    expect(() => buildMcpTransport({
      transport: 'stdio', command: '/bin/mcp', args: '"--unsafe"', environmentCredentials: '[]', runtimeSandbox: 'sandbox:version',
    })).toThrow(/string array/)
    expect(() => buildMcpTransport({
      transport: 'stdio', command: '/bin/mcp', args: '[]', environmentCredentials: '[{"name":"TOKEN","value":"secret"}]', runtimeSandbox: 'sandbox:version',
    })).toThrow(/credentialId/)
    expect(() => buildMcpTransport({
      transport: 'stdio', command: 'mcp-server', args: '[]', environmentCredentials: '[]', runtimeSandbox: 'sandbox:version',
    })).toThrow(/absolute executable path/)
    expect(() => buildMcpTransport({
      transport: 'stdio', command: '/bin/mcp', args: '[]', environmentCredentials: '[{"name":"TOKEN","credentialId":"one"},{"name":"TOKEN","credentialId":"two"}]', runtimeSandbox: 'sandbox:version',
    })).toThrow(/unique/)
  })
})
