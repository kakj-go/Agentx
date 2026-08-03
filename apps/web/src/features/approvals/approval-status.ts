export function approvalStatus(status: string): 'pending' | 'waiting' | 'completed' | 'inactive' {
  if (status === 'pending') return 'pending'
  if (status === 'claimed') return 'waiting'
  if (status === 'approved') return 'completed'
  return 'inactive'
}
