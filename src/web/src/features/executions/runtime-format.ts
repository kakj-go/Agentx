import { formatDateTime } from '../../shared/lib/locale-format'

export function formatRuntimeTimestamp(value: string, language?: string) {
  return formatDateTime(value, language)
}
