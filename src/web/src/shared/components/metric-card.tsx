import { Card } from '../ui/card'

type MetricCardProps = { label: string; value: string; suffix?: string; note?: string; noteTone?: 'success' | 'muted' }

export function MetricCard({ label, value, suffix, note = '', noteTone = 'success' }: MetricCardProps) {
  return (
    <Card className="rounded-none border-0 border-r border-border p-5 shadow-none last:border-r-0">
      <div className="flex items-center justify-between text-xs text-muted-foreground"><span>{label}</span><span className={noteTone === 'success' ? 'text-success' : 'text-muted-foreground'}>{note}</span></div>
      <strong className="mt-2.5 block text-2xl tracking-tight">{value} {suffix && <small className="text-[10px] font-normal text-muted-foreground">{suffix}</small>}</strong>
    </Card>
  )
}
