import type { ReactNode } from 'react'
import { Navigate, useLocation } from 'react-router-dom'

import { useAuth } from './providers/auth-provider'

export function Protected({ children }: { children: ReactNode }) {
  const { status } = useAuth(); const location = useLocation()
  if (status === 'loading') return <div className="grid min-h-screen place-items-center bg-background text-sm text-muted-foreground">Agentx…</div>
  if (status === 'setup') return <Navigate replace to="/setup" />
  if (status === 'password-change') return <Navigate replace to="/change-password" />
  if (status !== 'authenticated') return <Navigate replace state={{ from: location.pathname }} to="/login" />
  return children
}

export function PublicRoute({ kind, children }: { kind: 'setup' | 'login' | 'change-password'; children: ReactNode }) {
  const { status } = useAuth()
  if (status === 'loading') return <div className="grid min-h-screen place-items-center bg-background text-sm text-muted-foreground">Agentx…</div>
  if (status === 'authenticated') return <Navigate replace to="/" />
  if (status === 'setup' && kind !== 'setup') return <Navigate replace to="/setup" />
  if (status !== 'setup' && kind === 'setup') return <Navigate replace to="/login" />
  if (status === 'password-change' && kind !== 'change-password') return <Navigate replace to="/change-password" />
  if (status !== 'password-change' && kind === 'change-password') return <Navigate replace to="/login" />
  return children
}

export function RequirePermission({ permission, children }: { permission: string; children: ReactNode }) {
  const { status, hasPermission } = useAuth()
  if (status !== 'authenticated') return children
  return hasPermission(permission) ? children : <Navigate replace to="/403" />
}
