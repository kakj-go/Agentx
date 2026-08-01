import { ArrowUpRight, Bot, GitBranch, Plus, Wrench } from 'lucide-react'

import { Button } from '../shared/ui/button'
import { StatusBadge } from '../shared/ui/status-badge'

const metrics = [
  { label: '运行中', value: '18', suffix: '个执行', note: '实时' },
  { label: '今日执行', value: '1,284', suffix: '', note: '↑ 12.4%' },
  { label: '成功率', value: '98.7%', suffix: '', note: '↑ 1.8%' },
  { label: '今日成本', value: '¥ 428.60', suffix: '', note: '预算 62%' },
]

const workflows = [
  { name: '客户服务智能路由', detail: 'Agent · 8 个节点 · v17', status: '已发布', run: '2 分钟前', rate: '99.2%', icon: Bot },
  { name: '合同审查与人工审批', detail: 'Workflow · 12 个节点 · v8', status: '已发布', run: '18 分钟前', rate: '97.8%', icon: GitBranch },
  { name: '销售线索自动评分', detail: 'Agent · 6 个节点 · Draft', status: '草稿', run: '昨天 16:42', rate: '—', icon: Wrench },
]

export function DashboardPage() {
  return (
    <div className="mx-auto w-full max-w-[1480px] p-6 lg:p-8">
      <div className="flex items-end justify-between gap-4">
        <div>
          <p className="mb-2 text-[10px] font-semibold uppercase tracking-[0.14em] text-muted-foreground">Workspace overview</p>
          <h1 className="text-2xl font-bold tracking-tight">早上好，林晓</h1>
          <p className="mt-2 text-xs text-muted-foreground">这里是星海科技今天的 Workflow 运行概况。</p>
        </div>
        <Button><Plus className="size-4" />新建工作流</Button>
      </div>

      <section className="mt-7 grid overflow-hidden rounded-xl border border-border bg-surface shadow-sm sm:grid-cols-2 xl:grid-cols-4">
        {metrics.map((metric) => (
          <article className="border-b border-border p-5 last:border-b-0 sm:odd:border-r xl:border-b-0 xl:border-r xl:last:border-r-0" key={metric.label}>
            <div className="flex items-center justify-between text-xs text-muted-foreground"><span>{metric.label}</span><span className="text-[10px] text-success">{metric.note}</span></div>
            <strong className="mt-2.5 block text-2xl tracking-tight">{metric.value} <small className="text-[10px] font-normal text-muted-foreground">{metric.suffix}</small></strong>
          </article>
        ))}
      </section>

      <section className="mt-5 overflow-hidden rounded-xl border border-border bg-surface shadow-sm">
        <div className="flex h-15 items-center justify-between border-b border-border px-5">
          <h2 className="text-sm font-semibold">最近工作流</h2>
          <Button size="sm" variant="ghost">查看全部 <ArrowUpRight className="size-3.5" /></Button>
        </div>
        <div className="overflow-x-auto">
          <table className="w-full min-w-[720px] table-fixed text-left">
            <thead className="bg-muted/45 text-[10px] uppercase tracking-[0.08em] text-muted-foreground">
              <tr><th className="w-[44%] px-5 py-3 font-semibold">工作流</th><th className="px-5 py-3 font-semibold">状态</th><th className="px-5 py-3 font-semibold">最近运行</th><th className="px-5 py-3 font-semibold">成功率</th></tr>
            </thead>
            <tbody>
              {workflows.map((workflow) => {
                const Icon = workflow.icon
                return (
                  <tr className="border-t border-border first:border-t-0 hover:bg-muted/30" key={workflow.name}>
                    <td className="px-5 py-3.5"><div className="flex items-center gap-3"><span className="grid size-9 place-items-center rounded-lg bg-primary/10 text-primary"><Icon className="size-4" /></span><span><strong className="block text-xs">{workflow.name}</strong><span className="mt-1 block text-[10px] text-muted-foreground">{workflow.detail}</span></span></div></td>
                    <td className="px-5 py-3.5"><StatusBadge tone={workflow.status === '已发布' ? 'success' : 'neutral'}>{workflow.status}</StatusBadge></td>
                    <td className="px-5 py-3.5 text-xs text-muted-foreground">{workflow.run}</td>
                    <td className="px-5 py-3.5 text-xs text-muted-foreground">{workflow.rate}</td>
                  </tr>
                )
              })}
            </tbody>
          </table>
        </div>
      </section>
    </div>
  )
}

