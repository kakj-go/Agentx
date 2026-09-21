import { describe, expect, it } from 'vitest'

import { bytesToGb, bytesToKb, gbToBytes, isDigestReference, kbToBytes, sandboxVersionBody } from './sandbox-profile-form'

describe('sandbox profile units and image digests', () => {
  it('converts user-facing GB and KB to canonical bytes', () => {
    expect(gbToBytes('0.5')).toBe(536870912)
    expect(kbToBytes('1024')).toBe(1048576)
    expect(bytesToGb(536870912)).toBe('0.5')
    expect(bytesToKb(1048576)).toBe('1024')
  })

  it('accepts digest references and rejects tags or malformed digests', () => {
    expect(isDigestReference('registry.example:5000/runner@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa')).toBe(true)
    expect(isDigestReference('runner')).toBe(false)
    expect(isDigestReference('registry.example/runner:stable')).toBe(false)
    expect(isDigestReference('runner@sha256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA')).toBe(false)
    expect(isDigestReference('runner@sha256:short')).toBe(false)
  })

  it('sends canonical byte fields to the API', () => {
    expect(sandboxVersionBody({
      runner: 'python',
      imageDigest: 'opensandbox/code-interpreter@sha256:133a3c1720dd52291a019740c2987e7164ea6de79e23d8198798e58950ae2e6e',
      cpuMillis: '1000',
      memoryGb: '0.5',
      pidsLimit: '128',
      diskGb: '1',
      timeoutSeconds: '300',
      outputKb: '1024',
      allowTcpProxy: 'true',
    }, 'invalid json', 'invalid image')).toMatchObject({ memoryBytes: 536870912, diskBytes: 1073741824, outputLimitBytes: 1048576, networkPolicy: { defaultAction: 'deny', egressMode: 'tcp_proxy' } })
  })

  it('rejects non-digest image references before reaching the API', () => {
    expect(() => sandboxVersionBody({
      runner: 'python',
      imageDigest: 'runner:stable',
      cpuMillis: '1000',
      memoryGb: '0.5',
      pidsLimit: '128',
      diskGb: '1',
      timeoutSeconds: '300',
      outputKb: '1024',
      allowTcpProxy: 'false',
    }, 'invalid json', 'invalid image')).toThrow('invalid image')
  })
})
