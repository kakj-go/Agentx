import type { SandboxProfileVersion } from '../../shared/api/types'
import type { EntityFormField } from '../../shared/components/entity-form-dialog'

type Translate = (key: string) => string

export function sandboxVersionFields(t: Translate, version?: SandboxProfileVersion): EntityFormField[] {
  return [
    { name: 'runner', label: t('sandbox.runner'), type: 'select', defaultValue: version?.runner ?? 'python', options: ['python', 'javascript', 'shell', 'browser'].map((value) => ({ value, label: value })) },
    { name: 'imageDigest', label: t('sandbox.imageDigest'), required: true, defaultValue: version?.imageDigest ?? '', placeholder: 'registry.example/runner@sha256:…' },
    { name: 'cpuMillis', label: t('sandbox.cpuMillis'), type: 'number', required: true, min: 1, defaultValue: String(version?.cpuMillis ?? 1000) },
    { name: 'memoryBytes', label: t('sandbox.memoryBytes'), type: 'number', required: true, min: 1, defaultValue: String(version?.memoryBytes ?? 536870912) },
    { name: 'pidsLimit', label: t('sandbox.pidsLimit'), type: 'number', required: true, min: 1, defaultValue: String(version?.pidsLimit ?? 128) },
    { name: 'diskBytes', label: t('sandbox.diskBytes'), type: 'number', required: true, min: 1, defaultValue: String(version?.diskBytes ?? 1073741824) },
    { name: 'timeoutSeconds', label: t('sandbox.timeoutSeconds'), type: 'number', required: true, min: 60, max: 86400, defaultValue: String(version?.timeoutSeconds ?? 300) },
    { name: 'outputLimitBytes', label: t('sandbox.outputLimitBytes'), type: 'number', required: true, min: 1, defaultValue: String(version?.outputLimitBytes ?? 1048576) },
    { name: 'networkPolicy', label: t('sandbox.networkPolicy'), type: 'textarea', required: true, defaultValue: JSON.stringify(version?.networkPolicy ?? { defaultAction: 'deny', allow: [] }, null, 2) },
  ]
}

export function sandboxVersionBody(values: Record<string, string>, invalidJsonMessage: string) {
  let networkPolicy: unknown
  try { networkPolicy = JSON.parse(values.networkPolicy) as unknown } catch { throw new Error(invalidJsonMessage) }
  return {
    runner: values.runner,
    imageDigest: values.imageDigest.trim(),
    cpuMillis: positiveInteger(values.cpuMillis),
    memoryBytes: positiveInteger(values.memoryBytes),
    pidsLimit: positiveInteger(values.pidsLimit),
    diskBytes: positiveInteger(values.diskBytes),
    timeoutSeconds: positiveInteger(values.timeoutSeconds),
    outputLimitBytes: positiveInteger(values.outputLimitBytes),
    networkPolicy,
  }
}

function positiveInteger(value: string) {
  const parsed = Number(value)
  if (!Number.isSafeInteger(parsed) || parsed <= 0) throw new Error('Resource limits must be positive integers')
  return parsed
}
