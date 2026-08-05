import { BadgeCheck, Bot, Box, BrainCircuit, Clock3, Code2, Database, GitMerge, MemoryStick, MousePointerClick, Repeat2, Sparkles, Split, Wrench, type LucideIcon } from 'lucide-react'

const icons: Record<string, LucideIcon> = {
  'badge-check': BadgeCheck, bot: Bot, box: Box, 'brain-circuit': BrainCircuit, 'clock-3': Clock3, 'code-2': Code2, database: Database, 'git-merge': GitMerge, 'memory-stick': MemoryStick, 'mouse-pointer-click': MousePointerClick, 'repeat-2': Repeat2, sparkles: Sparkles, split: Split, wrench: Wrench,
}

export function NodeIcon({ iconKey, className }: { iconKey: string; className?: string }) {
  const Icon = icons[iconKey] ?? Box
  return <Icon className={className} />
}
