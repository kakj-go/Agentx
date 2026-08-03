import {
  Bell,
  Bot,
  Check,
  ChevronLeft,
  Globe2,
  LogOut,
  Menu,
  Monitor,
  Moon,
  Search,
  Settings,
  Sun,
  UserRound,
} from 'lucide-react'
import { useEffect, useMemo, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { NavLink, Outlet, useLocation, useNavigate } from 'react-router-dom'

import { navigationGroups, navigationItems } from '../../app/navigation'
import { useTheme } from '../../app/providers/theme-provider'
import { useAuth } from '../../app/providers/auth-provider'
import { cn } from '../../shared/lib/cn'
import { roleLabel } from '../../shared/lib/iam-labels'
import type { SupportedLocale, ThemePreference } from '../../shared/types/app'
import { Avatar } from '../../shared/ui/avatar'
import { Button } from '../../shared/ui/button'
import { Dialog, DialogContent } from '../../shared/ui/dialog'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '../../shared/ui/dropdown-menu'
import { Input } from '../../shared/ui/input'
import { Tooltip } from '../../shared/ui/tooltip'
import { useToast } from '../../shared/ui/toast'
import { apiRequest } from '../../shared/api/client'
import type { NotificationInbox } from '../../shared/api/types'

const sidebarStorageKey = 'agentx.sidebar.collapsed'

function pathTitleKey(pathname: string) {
  if (pathname.startsWith('/workflows/')) return 'nav.workflows'
  for (const item of navigationItems) if (item.path !== '/' && pathname.startsWith(`${item.path}/`)) return item.labelKey
  return navigationItems.find((item) => item.path === pathname)?.labelKey ?? 'nav.dashboard'
}

export function EnterpriseLayout() {
  const { t, i18n } = useTranslation()
  const { preference, resolvedTheme, setPreference } = useTheme()
  const { showToast } = useToast()
  const { user, logout, hasPermission } = useAuth()
  const location = useLocation()
  const navigate = useNavigate()
  const queryClient = useQueryClient()
  const [collapsed, setCollapsed] = useState(() => window.localStorage.getItem(sidebarStorageKey) === 'true')
  const [searchOpen, setSearchOpen] = useState(false)
  const [query, setQuery] = useState('')
  const tenantName = user?.companyName ?? t('app.name')
  const initials = (user?.displayName ?? user?.username ?? 'AX').slice(0, 2).toUpperCase()
  const notifications = useQuery({ enabled: hasPermission('notification:view'), queryKey: ['notifications'], queryFn: () => apiRequest<NotificationInbox>('/notifications'), refetchInterval: 30_000 })
  const readNotification = useMutation({ mutationFn: (id: string) => apiRequest(`/notifications/${id}/read`, { method: 'POST' }), onSuccess: async () => queryClient.invalidateQueries({ queryKey: ['notifications'] }) })
  const openNotification = (id: string, path: string) => { readNotification.mutate(id); navigate(path) }

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'k') {
        event.preventDefault()
        setSearchOpen(true)
      }
    }
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [])

  const visibleNavigationGroups = useMemo(() => navigationGroups.map((group) => ({
    ...group,
    items: group.items.filter((item) => !item.requiredPermission || hasPermission(item.requiredPermission)),
  })).filter((group) => group.items.length > 0), [hasPermission])
  const visibleNavigationItems = useMemo(() => visibleNavigationGroups.flatMap((group) => group.items), [visibleNavigationGroups])

  const searchResults = useMemo(() => {
    const normalized = query.trim().toLocaleLowerCase()
    if (!normalized) return visibleNavigationItems
    return visibleNavigationItems.filter((item) => `${t(item.labelKey)} ${item.keywords?.join(' ') ?? ''}`.toLocaleLowerCase().includes(normalized))
  }, [query, t, visibleNavigationItems])

  const toggleSidebar = () => {
    const next = !collapsed
    setCollapsed(next)
    window.localStorage.setItem(sidebarStorageKey, String(next))
  }

  const navigateFromMenu = (path: string) => {
    navigate(path)
    setSearchOpen(false)
    setQuery('')
  }

  return (
    <div className={cn('grid h-screen min-w-[1180px] overflow-hidden bg-background', collapsed ? 'grid-cols-[72px_1fr]' : 'grid-cols-[248px_1fr]')}>
      <aside className="flex min-w-0 flex-col border-r border-border bg-sidebar">
        <div className={cn('flex h-18 items-center gap-3 px-5', collapsed && 'justify-center px-0')}>
          <div className="grid size-9 shrink-0 place-items-center rounded-xl bg-primary text-primary-foreground shadow-sm"><Bot className="size-5" /></div>
          {!collapsed && <div><strong className="block text-[15px] tracking-tight">{t('app.name')}</strong><span className="text-[10px] font-medium uppercase tracking-[0.16em] text-muted-foreground">{t('app.subtitle')}</span></div>}
        </div>

        <nav className="mt-2 flex-1 overflow-y-auto px-2.5 pb-4">
          {visibleNavigationGroups.map((group) => (
            <div className="mb-4" key={group.labelKey}>
              {!collapsed && <p className="mb-1 px-2.5 text-[10px] font-semibold uppercase tracking-[0.12em] text-muted-foreground">{t(group.labelKey)}</p>}
              {group.items.map((item) => {
                const Icon = item.icon
                const link = (
                  <NavLink className={({ isActive }) => cn('my-0.5 flex h-10 items-center gap-3 rounded-lg px-2.5 text-[13px] text-muted-foreground transition-colors hover:bg-muted hover:text-foreground', isActive && 'bg-primary/10 font-semibold text-primary hover:bg-primary/10 hover:text-primary', collapsed && 'justify-center px-0')} end={item.path === '/'} to={item.path}>
                    <Icon className="size-4.5 shrink-0" />{!collapsed && <span className="truncate">{t(item.labelKey)}</span>}{!collapsed && item.badge && <span className="ml-auto rounded-full bg-muted px-2 py-0.5 text-[10px] text-muted-foreground">{item.badge}</span>}
                  </NavLink>
                )
                return collapsed ? <Tooltip content={t(item.labelKey)} key={item.path}>{link}</Tooltip> : <span className="contents" key={item.path}>{link}</span>
              })}
            </div>
          ))}
        </nav>

      </aside>

      <div className="flex min-w-0 flex-col">
        <header className="flex h-16 shrink-0 items-center gap-3 border-b border-border bg-surface/95 px-6">
          <Button aria-label={t(collapsed ? 'header.expand' : 'header.collapse')} onClick={toggleSidebar} size="icon" variant="ghost">{collapsed ? <Menu className="size-4" /> : <ChevronLeft className="size-4" />}</Button>
          <div className="text-xs text-muted-foreground">{tenantName}<span className="mx-2">/</span><strong className="text-foreground">{t(pathTitleKey(location.pathname))}</strong></div>
          <div className="flex-1" />
          <button className="flex h-9 w-80 items-center gap-2 rounded-lg border border-border bg-muted/50 px-3 text-xs text-muted-foreground outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-primary/25" onClick={() => setSearchOpen(true)}><Search className="size-4" /><span className="truncate">{t('header.searchPlaceholder')}</span><kbd className="ml-auto text-[10px]">⌘ K</kbd></button>

          <DropdownMenu>
            <DropdownMenuTrigger asChild><Button aria-label={t('common.language')} size="icon" variant="ghost"><Globe2 className="size-4" /></Button></DropdownMenuTrigger>
            <DropdownMenuContent align="end"><DropdownMenuLabel>{t('common.language')}</DropdownMenuLabel><DropdownMenuRadioGroup onValueChange={(value) => void i18n.changeLanguage(value as SupportedLocale)} value={i18n.resolvedLanguage === 'en-US' ? 'en-US' : 'zh-CN'}><DropdownMenuRadioItem value="zh-CN">{t('header.chinese')}</DropdownMenuRadioItem><DropdownMenuRadioItem value="en-US">{t('header.english')}</DropdownMenuRadioItem></DropdownMenuRadioGroup></DropdownMenuContent>
          </DropdownMenu>

          <DropdownMenu>
            <DropdownMenuTrigger asChild><Button aria-label={t('common.theme')} size="icon" variant="ghost">{resolvedTheme === 'dark' ? <Moon className="size-4" /> : <Sun className="size-4" />}</Button></DropdownMenuTrigger>
            <DropdownMenuContent align="end"><DropdownMenuLabel>{t('common.theme')}</DropdownMenuLabel><DropdownMenuRadioGroup onValueChange={(value) => setPreference(value as ThemePreference)} value={preference}><DropdownMenuRadioItem value="system"><Monitor className="size-4" />{t('header.systemTheme')}</DropdownMenuRadioItem><DropdownMenuRadioItem value="light"><Sun className="size-4" />{t('header.lightTheme')}</DropdownMenuRadioItem><DropdownMenuRadioItem value="dark"><Moon className="size-4" />{t('header.darkTheme')}</DropdownMenuRadioItem></DropdownMenuRadioGroup></DropdownMenuContent>
          </DropdownMenu>

          <DropdownMenu>
            <DropdownMenuTrigger asChild><Button aria-label={t('common.notifications')} className="relative" size="icon" variant="ghost"><Bell className="size-4" />{Boolean(notifications.data?.unreadCount) && <span className="absolute right-2 top-2 size-1.5 rounded-full bg-danger ring-2 ring-surface" />}</Button></DropdownMenuTrigger>
            <DropdownMenuContent align="end" className="w-96"><div className="flex items-center justify-between px-2.5 py-2"><strong className="text-sm">{t('common.notifications')}</strong><span className="text-[10px] text-muted-foreground">{t('header.unread', { count: notifications.data?.unreadCount ?? 0 })}</span></div><DropdownMenuSeparator />{notifications.data?.items.slice(0, 5).map((item) => <DropdownMenuItem className="items-start py-3" key={item.id} onSelect={() => openNotification(item.id, item.targetPath)}><span className={cn('mt-1 size-2 shrink-0 rounded-full', item.read && 'bg-border', !item.read && item.tone === 'warning' && 'bg-warning', !item.read && item.tone === 'success' && 'bg-success', !item.read && item.tone === 'danger' && 'bg-danger', !item.read && item.tone === 'primary' && 'bg-primary')} /><span><strong className="block text-xs">{t(item.titleKey, item.arguments as Record<string, string>)}</strong><span className="mt-1 block text-[10px] leading-4 text-muted-foreground">{t(item.bodyKey, item.arguments as Record<string, string>)}</span><span className="mt-1.5 block text-[10px] text-muted-foreground">{new Date(item.createdAt).toLocaleString()}</span></span></DropdownMenuItem>)}{!notifications.data?.items.length && <div className="px-4 py-8 text-center text-xs text-muted-foreground">{t('header.noNotifications')}</div>}<DropdownMenuSeparator /><DropdownMenuItem onSelect={() => navigate('/notifications')}>{t('common.viewAll')}</DropdownMenuItem></DropdownMenuContent>
          </DropdownMenu>

          <DropdownMenu>
            <DropdownMenuTrigger asChild><button aria-label={t('header.profile')} className="rounded-full outline-none focus-visible:ring-2 focus-visible:ring-primary/30"><Avatar initials={initials} tone="dark" /></button></DropdownMenuTrigger>
            <DropdownMenuContent align="end" className="w-72"><div className="flex items-center gap-3 px-2.5 py-3"><Avatar initials={initials} tone="dark" /><span><strong className="block text-sm">{user?.displayName}</strong><span className="text-[10px] text-muted-foreground">@{user?.username}</span></span></div><DropdownMenuSeparator /><dl className="grid grid-cols-[72px_1fr] gap-x-3 gap-y-2 px-3 py-2.5 text-[11px]"><dt className="text-muted-foreground">{t('header.company')}</dt><dd className="truncate font-medium">{user?.companyName}</dd><dt className="text-muted-foreground">{t('header.department')}</dt><dd className="truncate font-medium">{user?.departmentName}</dd><dt className="text-muted-foreground">{t('header.roles')}</dt><dd className="flex flex-wrap gap-1">{user?.roles.map((role) => <span className="rounded-md bg-primary/10 px-1.5 py-0.5 font-medium text-primary" key={role}>{roleLabel(t, role)}</span>)}</dd></dl><DropdownMenuSeparator /><DropdownMenuItem onSelect={() => showToast(t('common.comingSoon'))}><UserRound className="size-4" />{t('header.profile')}</DropdownMenuItem><DropdownMenuItem onSelect={() => showToast(t('common.comingSoon'))}><Settings className="size-4" />{t('header.preferences')}</DropdownMenuItem><DropdownMenuSeparator /><DropdownMenuItem className="text-danger" onSelect={() => void logout()}><LogOut className="size-4" />{t('header.logout')}</DropdownMenuItem></DropdownMenuContent>
          </DropdownMenu>
        </header>

        <main className="min-h-0 flex-1 overflow-auto"><Outlet context={{ tenantName }} /></main>
      </div>

      <Dialog onOpenChange={setSearchOpen} open={searchOpen}>
        <DialogContent description={t('header.searchHint')} title={t('common.search')}>
          <div className="flex h-14 items-center gap-3 border-b border-border px-4"><Search className="size-4 text-muted-foreground" /><Input autoFocus className="h-auto border-0 bg-transparent px-0 shadow-none focus:ring-0" onChange={(event) => setQuery(event.target.value)} placeholder={t('header.searchHint')} value={query} /></div>
          <div className="max-h-96 overflow-y-auto p-2">{searchResults.length === 0 ? <div className="p-8 text-center text-xs text-muted-foreground">{t('common.noResults')}</div> : searchResults.map((item) => { const Icon = item.icon; return <button className="flex h-11 w-full items-center gap-3 rounded-lg px-3 text-left text-xs text-muted-foreground hover:bg-muted hover:text-foreground" key={item.path} onClick={() => navigateFromMenu(item.path)}><Icon className="size-4 text-primary" /><span className="flex-1">{t(item.labelKey)}</span><Check className={cn('size-3.5', location.pathname === item.path ? 'text-success' : 'opacity-0')} /></button> })}</div>
        </DialogContent>
      </Dialog>
    </div>
  )
}
