import { fireEvent, render, screen } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { useState } from 'react'
import { describe, expect, it, vi } from 'vitest'

import { McpEnvironmentCredentials, McpRuntimeSandboxPicker } from './mcp-transport-fields'

vi.mock('./use-mcp-dependency-options', () => ({
  useMcpDependencyOptions: (_departmentId: string, resourceType: string) => ({
    options: resourceType === 'sandbox_profile'
      ? [{ value: 'sandbox-1', versionId: 'sandbox-version-2', label: 'Secure Sandbox', detail: 'isolated', accessState: 'authorized' }]
      : [{ value: 'credential-1', label: 'Provider token', detail: 'api_key', accessState: 'authorized' }],
    loading: false,
    error: null,
    authorize: vi.fn(),
    request: vi.fn(),
  }),
}))

describe('MCP structured stdio fields', () => {
  it('selects an exact Runtime Sandbox through the shared ResourcePicker', () => {
    const onChange = vi.fn()
    render(<MemoryRouter><McpRuntimeSandboxPicker departmentId="department-1" onChange={onChange} value="" /></MemoryRouter>)
    fireEvent.click(screen.getByRole('combobox'))
    fireEvent.click(screen.getByRole('option', { name: /Secure Sandbox/ }))
    expect(onChange).toHaveBeenCalledWith('sandbox-1:sandbox-version-2')
  })

  it('builds environment Credential references without accepting literal secrets', () => {
    function Fixture() {
      const [value, setValue] = useState('[]')
      return <McpEnvironmentCredentials departmentId="department-1" onChange={setValue} value={value} />
    }
    const view = render(<MemoryRouter><Fixture /></MemoryRouter>)
    fireEvent.click(screen.getByRole('button', { name: /Add environment Credential/i }))
    fireEvent.change(screen.getByRole('textbox', { name: /Environment variable name/i }), { target: { value: 'api_token' } })
    fireEvent.click(screen.getByRole('combobox'))
    fireEvent.click(screen.getByRole('option', { name: /Provider token/ }))
    const value = view.container.querySelector<HTMLInputElement>('input[name="environmentCredentials"]')?.value
    expect(JSON.parse(value ?? '[]')).toEqual([{ name: 'API_TOKEN', credentialId: 'credential-1' }])
    expect(value).not.toContain('secret')
  })
})
