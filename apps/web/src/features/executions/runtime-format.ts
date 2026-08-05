export function formatRuntimeTimestamp(value: string) {
  const timestamp = new Date(value)
  return Number.isNaN(timestamp.getTime()) ? '—' : timestamp.toLocaleString()
}
