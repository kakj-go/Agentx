import type { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'

import { Button, type ButtonProps } from '../ui/button'
import { useToast } from '../ui/toast'

type ComingSoonActionProps = Omit<ButtonProps, 'onClick'> & { children: ReactNode }

export function ComingSoonAction({ children, ...props }: ComingSoonActionProps) {
  const { t } = useTranslation()
  const { showToast } = useToast()
  return <Button onClick={() => showToast(t('common.comingSoon'))} {...props}>{children}</Button>
}
