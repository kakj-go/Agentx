import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useAuth } from '../../app/providers/auth-provider'
import { AuthForm, Field } from './auth-form'
import { AuthShell } from './auth-shell'
export function LoginPage(){const{t}=useTranslation();const{login}=useAuth();const[username,setUsername]=useState('');const[password,setPassword]=useState('');return <AuthShell eyebrow={t('auth.login.eyebrow')} title={t('auth.login.title')} description={t('auth.login.description')}><AuthForm submitLabel={t('auth.login.submit')} onSubmit={()=>login(username,password)}><Field autoComplete="username" label={t('auth.username')} onChange={e=>setUsername(e.target.value)} required value={username}/><Field autoComplete="current-password" label={t('auth.password')} onChange={e=>setPassword(e.target.value)} required type="password" value={password}/></AuthForm></AuthShell>}
