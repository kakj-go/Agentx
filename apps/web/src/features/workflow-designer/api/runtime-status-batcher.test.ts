import { afterEach, describe, expect, it, vi } from 'vitest'

import { RuntimeStatusBatcher } from './runtime-status-batcher'

afterEach(() => vi.useRealTimers())

describe('RuntimeStatusBatcher', () => {
  it('coalesces active states for 100ms and flushes terminal states immediately', () => {
    vi.useFakeTimers()
    const commit = vi.fn()
    const batcher = new RuntimeStatusBatcher(commit)
    batcher.merge([['node-a', 'queued'], ['node-b', 'running']])
    batcher.merge([['node-a', 'running']])
    expect(commit).not.toHaveBeenCalled()

    vi.advanceTimersByTime(99)
    expect(commit).not.toHaveBeenCalled()
    vi.advanceTimersByTime(1)
    expect(commit).toHaveBeenCalledOnce()
    expect(commit.mock.calls[0][0]).toEqual(new Map([['node-a', 'running'], ['node-b', 'running']]))

    batcher.merge([['node-a', 'succeeded']])
    expect(commit).toHaveBeenCalledTimes(2)
    expect(commit.mock.calls[1][0]).toEqual(new Map([['node-a', 'succeeded']]))
    batcher.dispose()
  })
})
