import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { useAuth } from '../../app/providers/auth-provider'
import { AuthForm, Field } from './auth-form'
import { AuthShell } from './auth-shell'

export function SetupPage() { const { t, i18n } = useTranslation(); const { setup } = useAuth(); const [companyName,setCompanyName]=useState('');const [username,setUsername]=useState('admin');const [displayName,setDisplayName]=useState('');const [password,setPassword]=useState('');
  return <AuthShell eyebrow={t('auth.setup.eyebrow')} title={t('auth.setup.title')} description={t('auth.setup.description')}><AuthForm submitLabel={t('auth.setup.submit')} onSubmit={() => setup({companyName,adminUsername:username,adminDisplayName:displayName,password,locale:i18n.language,timezone:Intl.DateTimeFormat().resolvedOptions().timeZone})}><Field label={t('auth.companyName')} onChange={e=>setCompanyName(e.target.value)} required value={companyName}/><div className="grid grid-cols-2 gap-4"><Field autoComplete="username" label={t('auth.username')} onChange={e=>setUsername(e.target.value)} required value={username}/><Field label={t('auth.displayName')} onChange={e=>setDisplayName(e.target.value)} required value={displayName}/></div><Field autoComplete="new-password" label={t('auth.password')} minLength={12} onChange={e=>setPassword(e.target.value)} required type="password" value={password}/><p className="text-[11px] text-muted-foreground">{t('auth.passwordHint')}</p></AuthForm></AuthShell> }
