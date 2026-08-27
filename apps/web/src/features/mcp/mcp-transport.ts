import type { McpServer, SandboxProfile } from '../../shared/api/types'

export type McpTransport = McpServer['transport']

export function buildMcpTransport(values: Record<string, string>): McpTransport {
  if (values.transport === 'stdio') {
    const args = parseStringArray(values.args || '[]', 'args')
    const environmentCredentialRefs = parseEnvironment(values.environmentCredentials || '[]')
    const [resourceId, resourceVersionId] = (values.runtimeSandbox || '').split(':')
    if (!values.command.trim().startsWith('/')) throw new Error('stdio command must be an absolute executable path')
    if (!resourceId || !resourceVersionId) throw new Error('stdio Runtime Sandbox exact version is required')
    return {
      kind: 'stdio',
      command: values.command.trim(),
      args,
      environmentCredentialRefs,
      runtimeSandbox: { resourceId, resourceVersionId },
    }
  }
  if (!values.endpoint.trim()) throw new Error('MCP endpoint is required')
  return {
    kind: values.transport === 'sse' ? 'sse' : 'streamable_http',
    endpoint: values.endpoint.trim(),
    bearerCredentialId: values.credential || null,
  }
}

export function transportKind(transport: McpTransport) { return transport.kind }

export function transportEndpoint(transport: McpTransport) {
  return transport.kind === 'stdio' ? transport.command : transport.endpoint
}

export function transportCredential(transport: McpTransport) {
  return transport.kind === 'stdio' ? '' : (transport.bearerCredentialId ?? '')
}

export function transportArgs(transport: McpTransport) {
  return transport.kind === 'stdio' ? JSON.stringify(transport.args, null, 2) : '[]'
}

export function transportEnvironment(transport: McpTransport) {
  return transport.kind === 'stdio' ? JSON.stringify(transport.environmentCredentialRefs, null, 2) : '[]'
}

export function transportSandbox(transport: McpTransport) {
  return transport.kind === 'stdio'
    ? `${transport.runtimeSandbox.resourceId}:${transport.runtimeSandbox.resourceVersionId}`
    : ''
}

export function sandboxVersionOptions(profiles: SandboxProfile[]) {
  return profiles.flatMap((profile) => profile.versions.map((version) => ({
    value: `${profile.id}:${version.id}`,
    label: `${profile.name} · v${version.versionNumber}`,
  })))
}

function parseStringArray(raw: string, field: string) {
  let value: unknown
  try { value = JSON.parse(raw) as unknown } catch { throw new Error(`${field} must be valid JSON`) }
  if (!Array.isArray(value) || value.some((item) => typeof item !== 'string')) throw new Error(`${field} must be a JSON string array`)
  return value
}

function parseEnvironment(raw: string) {
  let value: unknown
  try { value = JSON.parse(raw) as unknown } catch { throw new Error('environment credentials must be valid JSON') }
  if (!Array.isArray(value) || value.some((item) => {
    const reference = item as { name?: unknown; credentialId?: unknown }
    return !reference || typeof reference.name !== 'string' || !/^[A-Z_][A-Z0-9_]{0,127}$/.test(reference.name)
      || typeof reference.credentialId !== 'string' || !reference.credentialId
  })) throw new Error('environment credentials must be an array of name and credentialId references')
  const references = value as Array<{ name: string; credentialId: string }>
  if (new Set(references.map((reference) => reference.name)).size !== references.length) {
    throw new Error('environment credential names must be unique')
  }
  return references
}
