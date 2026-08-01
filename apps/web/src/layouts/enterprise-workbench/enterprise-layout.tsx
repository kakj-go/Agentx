import {
  AppWindow,
  Bell,
  Blocks,
  Bot,
  BrainCircuit,
  ChevronLeft,
  CircleGauge,
  Database,
  FlaskConical,
  KeyRound,
  Library,
  MemoryStick,
  Menu,
  Moon,
  Play,
  Search,
  ShieldCheck,
  Sun,
  UserRound,
  UsersRound,
  Wrench,
} from 'lucide-react'
import { useEffect, useState, type ComponentType } from 'react'
import { NavLink, Outlet, useLocation } from 'react-router-dom'

import { cn } from '../../shared/lib/cn'
import { Button } from '../../shared/ui/button'

type MenuItem = {
  label: string
  path: string
  icon: ComponentType<{ className?: string }>
  badge?: string
}

const menuGroups: Array<{ label: string; items: MenuItem[] }> = [
  {
    label: '工作空间',
    items: [
      { label: '总览', path: '/', icon: CircleGauge },
      { label: '工作流', path: '/workflows', icon: Blocks, badge: '24' },
      { label: '应用', path: '/applications', icon: AppWindow },
      { label: 'Playground', path: '/playground', icon: Play },
    ],
  },
  {
    label: '运行与质量',
    items: [
      { label: '执行记录', path: '/executions', icon: CircleGauge },
      { label: '待审批', path: '/approvals', icon: ShieldCheck, badge: '3' },
      { label: '测试集', path: '/datasets', icon: Database },
      { label: '评测报告', path: '/evaluations', icon: FlaskConical },
    ],
  },
  {
    label: '资源',
    items: [
      { label: '模型服务', path: '/models', icon: BrainCircuit },
      { label: '工具与连接', path: '/tools', icon: Wrench },
      { label: '知识库', path: '/knowledge', icon: Library },
      { label: 'Memory', path: '/memory', icon: MemoryStick },
    ],
  },
  {
    label: '组织',
    items: [
      { label: '部门与用户', path: '/organization', icon: UsersRound },
      { label: '角色权限', path: '/roles', icon: KeyRound },
    ],
  },
]

function currentTitle(pathname: string) {
  return menuGroups.flatMap((group) => group.items).find((item) => item.path === pathname)?.label ?? '总览'
}

export function EnterpriseLayout() {
  const [collapsed, setCollapsed] = useState(false)
  const [dark, setDark] = useState(false)
  const location = useLocation()

  useEffect(() => {
    document.documentElement.classList.toggle('dark', dark)
  }, [dark])

  return (
    <div className={cn('grid h-screen overflow-hidden bg-background', collapsed ? 'grid-cols-[72px_1fr]' : 'grid-cols-[248px_1fr]')}>
      <aside className="flex min-w-0 flex-col border-r border-border bg-sidebar transition-[width]">
        <div className={cn('flex h-18 items-center gap-3 px-5', collapsed && 'justify-center px-0')}>
          <div className="grid size-9 shrink-0 place-items-center rounded-xl bg-primary text-primary-foreground shadow-sm">
            <Bot className="size-5" />
          </div>
          {!collapsed && (
            <div>
              <strong className="block text-[15px] tracking-tight">Agentx</strong>
              <span className="text-[10px] font-medium uppercase tracking-[0.16em] text-muted-foreground">Workflow Cloud</span>
            </div>
          )}
        </div>

        <button className={cn('mx-3 flex h-13 items-center gap-3 rounded-xl border border-border bg-muted/45 px-3 text-left', collapsed && 'justify-center px-0')}>
          <span className="grid size-7 shrink-0 place-items-center rounded-lg bg-primary/10 text-[11px] font-bold text-primary">AC</span>
          {!collapsed && <span className="min-w-0 flex-1"><strong className="block truncate text-xs">星海科技</strong><span className="block truncate text-[10px] text-muted-foreground">Enterprise Workspace</span></span>}
        </button>

        <nav className="mt-4 flex-1 overflow-y-auto px-2.5 pb-4">
          {menuGroups.map((group) => (
            <div className="mb-4" key={group.label}>
              {!collapsed && <p className="mb-1 px-2.5 text-[10px] font-semibold uppercase tracking-[0.12em] text-muted-foreground">{group.label}</p>}
              {group.items.map((item) => {
                const Icon = item.icon
                return (
                  <NavLink
                    className={({ isActive }) =>
                      cn(
                        'my-0.5 flex h-10 items-center gap-3 rounded-lg px-2.5 text-[13px] text-muted-foreground transition-colors hover:bg-muted hover:text-foreground',
                        isActive && 'bg-primary/10 font-semibold text-primary hover:bg-primary/10 hover:text-primary',
                        collapsed && 'justify-center px-0',
                      )
                    }
                    end={item.path === '/'}
                    key={item.path}
                    title={collapsed ? item.label : undefined}
                    to={item.path}
                  >
                    <Icon className="size-4.5 shrink-0" />
                    {!collapsed && <span className="truncate">{item.label}</span>}
                    {!collapsed && item.badge && <span className="ml-auto rounded-full bg-muted px-2 py-0.5 text-[10px] text-muted-foreground">{item.badge}</span>}
                  </NavLink>
                )
              })}
            </div>
          ))}
        </nav>

        <div className={cn('m-3 flex items-center gap-3 border-t border-border px-2 pt-3', collapsed && 'justify-center px-0')}>
          <div className="grid size-8 shrink-0 place-items-center rounded-full bg-foreground text-[10px] font-bold text-background">LX</div>
          {!collapsed && <div className="min-w-0 flex-1"><strong className="block truncate text-xs">林晓</strong><span className="text-[10px] text-muted-foreground">平台管理员</span></div>}
        </div>
      </aside>

      <div className="flex min-w-0 flex-col">
        <header className="flex h-16 shrink-0 items-center gap-3 border-b border-border bg-surface/95 px-6">
          <Button aria-label="折叠菜单" onClick={() => setCollapsed((value) => !value)} size="icon" variant="ghost">
            {collapsed ? <Menu className="size-4" /> : <ChevronLeft className="size-4" />}
          </Button>
          <div className="text-xs text-muted-foreground">星海科技 <span className="mx-2">/</span> <strong className="text-foreground">{currentTitle(location.pathname)}</strong></div>
          <div className="flex-1" />
          <div className="hidden h-9 w-72 items-center gap-2 rounded-lg border border-border bg-muted/50 px-3 text-xs text-muted-foreground lg:flex">
            <Search className="size-4" />
            <span>搜索工作流、应用或执行记录</span>
            <kbd className="ml-auto text-[10px]">⌘ K</kbd>
          </div>
          <Button aria-label="切换主题" onClick={() => setDark((value) => !value)} size="icon" variant="ghost">
            {dark ? <Sun className="size-4" /> : <Moon className="size-4" />}
          </Button>
          <Button aria-label="通知" size="icon" variant="ghost"><Bell className="size-4" /></Button>
          <Button aria-label="用户菜单" size="icon" variant="ghost"><UserRound className="size-4" /></Button>
        </header>

        <main className="min-h-0 flex-1 overflow-auto">
          <Outlet />
        </main>
      </div>
    </div>
  )
}

