import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useAuth } from '../../app/providers/auth-provider'
import { AuthForm, Field } from './auth-form'
import { AuthShell } from './auth-shell'
export function ChangePasswordPage(){const{t}=useTranslation();const{changePassword}=useAuth();const[password,setPassword]=useState('');const[confirm,setConfirm]=useState('');return <AuthShell eyebrow={t('auth.change.eyebrow')} title={t('auth.change.title')} description={t('auth.change.description')}><AuthForm submitLabel={t('auth.change.submit')} onSubmit={()=>{if(password!==confirm)throw new Error(t('auth.passwordMismatch'));return changePassword(password)}}><Field autoComplete="new-password" label={t('auth.newPassword')} minLength={12} onChange={e=>setPassword(e.target.value)} required type="password" value={password}/><Field autoComplete="new-password" label={t('auth.confirmPassword')} minLength={12} onChange={e=>setConfirm(e.target.value)} required type="password" value={confirm}/></AuthForm></AuthShell>}
