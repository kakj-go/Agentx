import { beforeEach, describe, expect, it, vi } from 'vitest'

import { apiRequest } from '../../shared/api/client'
import { loadMcpDependencyOptions } from './use-mcp-dependency-options'

vi.mock('../../shared/api/client', () => ({ apiRequest: vi.fn(), jsonBody: (value: unknown) => JSON.stringify(value) }))

describe('MCP dependency authorization options', () => {
  beforeEach(() => vi.mocked(apiRequest).mockReset())

  it('loads all department-scoped pages and preserves exact versions and access states', async () => {
    vi.mocked(apiRequest)
      .mockResolvedValueOnce({
        items: [{ id: 'sandbox-1', name: 'Sandbox 1', detail: 'digest-1', status: 'active', resourceVersionId: 'version-1', accessState: 'pending', pendingRequestId: 'request-1' }],
        page: 1, pageSize: 100, total: 2,
      })
      .mockResolvedValueOnce({
        items: [{ id: 'sandbox-2', name: 'Sandbox 2', detail: 'digest-2', status: 'active', resourceVersionId: 'version-2', accessState: 'authorized' }],
        page: 2, pageSize: 100, total: 2,
      })
    const options = await loadMcpDependencyOptions('department-1', 'sandbox_profile')
    expect(apiRequest).toHaveBeenNthCalledWith(1, '/departments/department-1/resource-options?resourceType=sandbox_profile&operation=use&page=1&pageSize=100')
    expect(apiRequest).toHaveBeenNthCalledWith(2, '/departments/department-1/resource-options?resourceType=sandbox_profile&operation=use&page=2&pageSize=100')
    expect(options).toEqual([
      expect.objectContaining({ value: 'sandbox-1', versionId: 'version-1', accessState: 'pending', pendingRequestId: 'request-1' }),
      expect.objectContaining({ value: 'sandbox-2', versionId: 'version-2', accessState: 'authorized' }),
    ])
  })
})
