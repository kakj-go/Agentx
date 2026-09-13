export function formatCost(micros: number | null | undefined, currency: string | null | undefined) {
  if (micros == null || !currency) return '—'
  try {
    return new Intl.NumberFormat(undefined, {
      style: 'currency',
      currency,
      minimumFractionDigits: 6,
      maximumFractionDigits: 6,
    }).format(micros / 1_000_000)
  } catch {
    return `${currency} ${(micros / 1_000_000).toFixed(6)}`
  }
}
