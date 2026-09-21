import type { SandboxProfileVersion } from '../../shared/api/types'
import type { EntityFormField } from '../../shared/components/entity-form-dialog'

type Translate = (key: string) => string

export function sandboxVersionFields(t: Translate, version?: SandboxProfileVersion): EntityFormField[] {
  return [
    { name: 'runner', label: t('sandbox.runner'), type: 'select', defaultValue: version?.runner ?? 'python', required: true, options: ['python', 'javascript', 'shell', 'browser'].map((value) => ({ value, label: value })) },
    { name: 'imageDigest', label: t('sandbox.imageTag'), required: true, defaultValue: version?.imageDigest ?? '', placeholder: 'registry.example/runner@sha256:<64位十六进制摘要>' },
    { name: 'cpuMillis', label: t('sandbox.cpuMillis'), type: 'number', required: true, min: 1, defaultValue: String(version?.cpuMillis ?? 1000) },
    { name: 'memoryGb', label: t('sandbox.memoryGb'), type: 'number', required: true, min: 0.001, step: 'any', defaultValue: bytesToGb(version?.memoryBytes ?? 536870912) },
    { name: 'pidsLimit', label: t('sandbox.pidsLimit'), type: 'number', required: true, min: 1, defaultValue: String(version?.pidsLimit ?? 128) },
    { name: 'diskGb', label: t('sandbox.diskGb'), type: 'number', required: true, min: 0.001, step: 'any', defaultValue: bytesToGb(version?.diskBytes ?? 1073741824) },
    { name: 'timeoutSeconds', label: t('sandbox.timeoutSeconds'), type: 'number', required: true, min: 60, max: 86400, defaultValue: String(version?.timeoutSeconds ?? 300) },
    { name: 'outputKb', label: t('sandbox.outputKb'), type: 'number', required: true, min: 0.001, step: 'any', defaultValue: bytesToKb(version?.outputLimitBytes ?? 1048576) },
    { name: 'allowTcpProxy', label: t('sandbox.allowTcpProxy'), type: 'checkbox', defaultValue: String(tcpProxyEnabled(version?.networkPolicy)), description: t('sandbox.allowTcpProxyWarning') },
  ]
}

export function sandboxVersionBody(values: Record<string, string>, _invalidJsonMessage: string, invalidImageMessage: string) {
  if (!isDigestReference(values.imageDigest)) throw new Error(invalidImageMessage)
  return {
    runner: values.runner,
    imageDigest: values.imageDigest.trim(),
    cpuMillis: positiveInteger(values.cpuMillis),
    memoryBytes: gbToBytes(values.memoryGb),
    pidsLimit: positiveInteger(values.pidsLimit),
    diskBytes: gbToBytes(values.diskGb),
    timeoutSeconds: positiveInteger(values.timeoutSeconds),
    outputLimitBytes: kbToBytes(values.outputKb),
    networkPolicy: { defaultAction: 'deny', egressMode: values.allowTcpProxy === 'true' ? 'tcp_proxy' : 'none' },
  }
}

function tcpProxyEnabled(value: unknown) {
  return Boolean(value && typeof value === 'object' && 'egressMode' in value && value.egressMode === 'tcp_proxy')
}

function positiveInteger(value: string) {
  const parsed = Number(value)
  if (!Number.isSafeInteger(parsed) || parsed <= 0) throw new Error('Resource limits must be positive integers')
  return parsed
}

function positiveBytes(value: string, multiplier: number) {
  const parsed = Number(value)
  const bytes = Math.round(parsed * multiplier)
  if (!Number.isFinite(parsed) || parsed <= 0 || !Number.isSafeInteger(bytes) || bytes <= 0) throw new Error('Resource limits must be positive numbers')
  return bytes
}

export function gbToBytes(value: string) { return positiveBytes(value, 1024 ** 3) }
export function kbToBytes(value: string) { return positiveBytes(value, 1024) }
export function bytesToGb(value: number) { return (value / (1024 ** 3)).toFixed(3).replace(/\.?(0+)$/, '') }
export function bytesToKb(value: number) { return (value / 1024).toFixed(3).replace(/\.?(0+)$/, '') }

export function isDigestReference(value: string) {
  const normalized = value.trim()
  const separator = normalized.lastIndexOf('@sha256:')
  if (separator <= 0 || /\s/.test(normalized)) return false
  const image = normalized.slice(0, separator)
  const digest = normalized.slice(separator + '@sha256:'.length)
  return !image.includes('@') && digest.length === 64 && /^[0-9a-f]+$/.test(digest)
}
