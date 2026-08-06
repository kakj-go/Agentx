import { describe, expect, it } from 'vitest'

import { bytesToGb, bytesToKb, gbToBytes, isTaggedImage, kbToBytes, sandboxVersionBody } from './sandbox-profile-form'

describe('sandbox profile units and image tags', () => {
  it('converts user-facing GB and KB to canonical bytes', () => {
    expect(gbToBytes('0.5')).toBe(536870912)
    expect(kbToBytes('1024')).toBe(1048576)
    expect(bytesToGb(536870912)).toBe('0.5')
    expect(bytesToKb(1048576)).toBe('1024')
  })

  it('accepts tagged image references and rejects digest references', () => {
    expect(isTaggedImage('registry.example:5000/runner:stable')).toBe(true)
    expect(isTaggedImage('runner')).toBe(false)
    expect(isTaggedImage('runner@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa')).toBe(false)
  })

  it('sends canonical byte fields to the API', () => {
    expect(sandboxVersionBody({
      runner: 'python',
      imageDigest: 'runner:stable',
      cpuMillis: '1000',
      memoryGb: '0.5',
      pidsLimit: '128',
      diskGb: '1',
      timeoutSeconds: '300',
      outputKb: '1024',
      networkPolicy: '{"defaultAction":"deny"}',
    }, 'invalid json', 'invalid image')).toMatchObject({ memoryBytes: 536870912, diskBytes: 1073741824, outputLimitBytes: 1048576 })
  })
})
