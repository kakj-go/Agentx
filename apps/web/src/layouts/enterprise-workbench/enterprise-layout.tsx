import {
  Bell,
  Bot,
  Check,
  ChevronDown,
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
import { useTranslation } from 'react-i18next'
import { NavLink, Outlet, useLocation, useNavigate } from 'react-router-dom'

import { navigationGroups, navigationItems, notifications } from '../../app/navigation'
import { useTheme } from '../../app/providers/theme-provider'
import { cn } from '../../shared/lib/cn'
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

const tenants = [
  { id: 'xinghai', nameKey: 'tenants.xinghai', subKey: 'tenants.xinghaiSub', initials: 'AC' },
  { id: 'beidou', nameKey: 'tenants.beidou', subKey: 'tenants.beidouSub', initials: 'BD' },
]

const sidebarStorageKey = 'agentx.sidebar.collapsed'
const tenantStorageKey = 'agentx.tenantId'

function pathTitleKey(pathname: string) {
  if (pathname.startsWith('/workflows/')) return 'nav.workflows'
  return navigationItems.find((item) => item.path === pathname)?.labelKey ?? 'nav.dashboard'
}

export function EnterpriseLayout() {
  const { t, i18n } = useTranslation()
  const { preference, resolvedTheme, setPreference } = useTheme()
  const { showToast } = useToast()
  const location = useLocation()
  const navigate = useNavigate()
  const [collapsed, setCollapsed] = useState(() => window.localStorage.getItem(sidebarStorageKey) === 'true')
  const [tenantId, setTenantId] = useState(() => window.localStorage.getItem(tenantStorageKey) ?? 'xinghai')
  const [searchOpen, setSearchOpen] = useState(false)
  const [query, setQuery] = useState('')
  const tenant = tenants.find((item) => item.id === tenantId) ?? tenants[0]
  const tenantName = t(tenant.nameKey)

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

  const searchResults = useMemo(() => {
    const normalized = query.trim().toLocaleLowerCase()
    if (!normalized) return navigationItems
    return navigationItems.filter((item) => `${t(item.labelKey)} ${item.keywords?.join(' ') ?? ''}`.toLocaleLowerCase().includes(normalized))
  }, [query, t])

  const toggleSidebar = () => {
    const next = !collapsed
    setCollapsed(next)
    window.localStorage.setItem(sidebarStorageKey, String(next))
  }

  const changeTenant = (next: string) => {
    setTenantId(next)
    window.localStorage.setItem(tenantStorageKey, next)
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

        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <button className={cn('mx-3 flex h-13 items-center gap-3 rounded-xl border border-border bg-muted/45 px-3 text-left outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-primary/25', collapsed && 'justify-center px-0')}>
              <Avatar className="size-7 rounded-lg" initials={tenant.initials} />
              {!collapsed && <><span className="min-w-0 flex-1"><strong className="block truncate text-xs">{tenantName}</strong><span className="block truncate text-[10px] text-muted-foreground">{t(tenant.subKey)}</span></span><ChevronDown className="size-3.5 text-muted-foreground" /></>}
            </button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="start" className="w-64">
            <DropdownMenuLabel>{t('header.tenant')}</DropdownMenuLabel>
            <DropdownMenuRadioGroup onValueChange={changeTenant} value={tenantId}>
              {tenants.map((item) => <DropdownMenuRadioItem key={item.id} value={item.id}><Avatar className="size-7 rounded-lg" initials={item.initials} /><span><strong className="block text-xs">{t(item.nameKey)}</strong><span className="text-[10px] text-muted-foreground">{t(item.subKey)}</span></span></DropdownMenuRadioItem>)}
            </DropdownMenuRadioGroup>
          </DropdownMenuContent>
        </DropdownMenu>

        <nav className="mt-4 flex-1 overflow-y-auto px-2.5 pb-4">
          {navigationGroups.map((group) => (
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

        <div className={cn('m-3 flex items-center gap-3 border-t border-border px-2 pt-3', collapsed && 'justify-center px-0')}><Avatar initials="LX" tone="dark" />{!collapsed && <div className="min-w-0 flex-1"><strong className="block truncate text-xs">{t('mocks.user.lin')}</strong><span className="text-[10px] text-muted-foreground">{t('header.admin')}</span></div>}</div>
      </aside>

      <div className="flex min-w-0 flex-col">
        <header className="flex h-16 shrink-0 items-center gap-3 border-b border-border bg-surface/95 px-6">
          <Button aria-label={t(collapsed ? 'header.expand' : 'header.collapse')} onClick={toggleSidebar} size="icon" variant="ghost">{collapsed ? <Menu className="size-4" /> : <ChevronLeft className="size-4" />}</Button>
          <div className="text-xs text-muted-foreground">{tenantName}<span className="mx-2">/</span><strong className="text-foreground">{t(pathTitleKey(location.pathname))}</strong></div>
          <div className="flex-1" />
          <button className="flex h-9 w-80 items-center gap-2 rounded-lg border border-border bg-muted/50 px-3 text-xs text-muted-foreground outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-primary/25" onClick={() => setSearchOpen(true)}><Search className="size-4" /><span className="truncate">{t('header.searchPlaceholder')}</span><kbd className="ml-auto text-[10px]">⌘ K</kbd></button>

          <DropdownMenu>
            <DropdownMenuTrigger asChild><Button aria-label={t('common.language')} size="icon" variant="ghost"><Globe2 className="size-4" /></Button></DropdownMenuTrigger>
            <DropdownMenuContent align="end"><DropdownMenuLabel>{t('common.language')}</DropdownMenuLabel><DropdownMenuRadioGroup onValueChange={(value) => void i18n.changeLanguage(value as SupportedLocale)} value={i18n.language}><DropdownMenuRadioItem value="zh-CN">{t('header.chinese')}</DropdownMenuRadioItem><DropdownMenuRadioItem value="en-US">{t('header.english')}</DropdownMenuRadioItem></DropdownMenuRadioGroup></DropdownMenuContent>
          </DropdownMenu>

          <DropdownMenu>
            <DropdownMenuTrigger asChild><Button aria-label={t('common.theme')} size="icon" variant="ghost">{resolvedTheme === 'dark' ? <Moon className="size-4" /> : <Sun className="size-4" />}</Button></DropdownMenuTrigger>
            <DropdownMenuContent align="end"><DropdownMenuLabel>{t('common.theme')}</DropdownMenuLabel><DropdownMenuRadioGroup onValueChange={(value) => setPreference(value as ThemePreference)} value={preference}><DropdownMenuRadioItem value="system"><Monitor className="size-4" />{t('header.systemTheme')}</DropdownMenuRadioItem><DropdownMenuRadioItem value="light"><Sun className="size-4" />{t('header.lightTheme')}</DropdownMenuRadioItem><DropdownMenuRadioItem value="dark"><Moon className="size-4" />{t('header.darkTheme')}</DropdownMenuRadioItem></DropdownMenuRadioGroup></DropdownMenuContent>
          </DropdownMenu>

          <DropdownMenu>
            <DropdownMenuTrigger asChild><Button aria-label={t('common.notifications')} className="relative" size="icon" variant="ghost"><Bell className="size-4" /><span className="absolute right-2 top-2 size-1.5 rounded-full bg-danger ring-2 ring-surface" /></Button></DropdownMenuTrigger>
            <DropdownMenuContent align="end" className="w-96"><div className="flex items-center justify-between px-2.5 py-2"><strong className="text-sm">{t('common.notifications')}</strong><span className="text-[10px] text-muted-foreground">{t('header.unread', { count: notifications.length })}</span></div><DropdownMenuSeparator />{notifications.map((item) => <DropdownMenuItem className="items-start py-3" key={item.id} onSelect={() => navigate(item.path)}><span className={cn('mt-1 size-2 shrink-0 rounded-full', item.tone === 'warning' && 'bg-warning', item.tone === 'success' && 'bg-success', item.tone === 'primary' && 'bg-primary')} /><span><strong className="block text-xs">{t(item.titleKey)}</strong><span className="mt-1 block text-[10px] leading-4 text-muted-foreground">{t(item.descriptionKey)}</span><span className="mt-1.5 block text-[10px] text-muted-foreground">{t(item.timeKey)}</span></span></DropdownMenuItem>)}</DropdownMenuContent>
          </DropdownMenu>

          <DropdownMenu>
            <DropdownMenuTrigger asChild><button aria-label={t('header.profile')} className="rounded-full outline-none focus-visible:ring-2 focus-visible:ring-primary/30"><Avatar initials="LX" tone="dark" /></button></DropdownMenuTrigger>
            <DropdownMenuContent align="end"><div className="flex items-center gap-3 px-2.5 py-2"><Avatar initials="LX" tone="dark" /><span><strong className="block text-xs">{t('mocks.user.lin')}</strong><span className="text-[10px] text-muted-foreground">{t('header.admin')}</span></span></div><DropdownMenuSeparator /><DropdownMenuItem onSelect={() => showToast(t('common.comingSoon'))}><UserRound className="size-4" />{t('header.profile')}</DropdownMenuItem><DropdownMenuItem onSelect={() => showToast(t('common.comingSoon'))}><Settings className="size-4" />{t('header.preferences')}</DropdownMenuItem><DropdownMenuSeparator /><DropdownMenuItem className="text-danger" onSelect={() => showToast(t('common.comingSoon'))}><LogOut className="size-4" />{t('header.logout')}</DropdownMenuItem></DropdownMenuContent>
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
