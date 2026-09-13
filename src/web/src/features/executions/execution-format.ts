export function executionStatus(status: string): 'success' | 'running' | 'waiting' | 'failed' | 'pending' | 'inactive' {
  if (status === 'succeeded') return 'success'
  if (status === 'running') return 'running'
  if (status.startsWith('waiting') || status === 'suspended') return 'waiting'
  if (status === 'failed' || status === 'timed_out') return 'failed'
  if (status === 'cancelled') return 'inactive'
  return 'pending'
}

export const formatCost = formatCurrencyCost
import { formatCost as formatCurrencyCost } from '../../shared/lib/cost-format'
