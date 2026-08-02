import {
  AppWindow,
  Blocks,
  BrainCircuit,
  CircleGauge,
  Database,
  FlaskConical,
  KeyRound,
  Library,
  MemoryStick,
  Play,
  ShieldCheck,
  Sparkles,
  UsersRound,
  Wrench,
} from 'lucide-react'

import type { NavigationGroup, NotificationItem } from '../shared/types/app'

export const navigationGroups: NavigationGroup[] = [
  {
    labelKey: 'nav.groups.workspace',
    items: [
      { labelKey: 'nav.dashboard', path: '/', icon: CircleGauge, keywords: ['overview', 'home'] },
      { labelKey: 'nav.workflows', path: '/workflows', icon: Blocks, badge: '24', keywords: ['agent', 'flow'] },
      { labelKey: 'nav.applications', path: '/applications', icon: AppWindow, keywords: ['deployment', 'api'] },
      { labelKey: 'nav.playground', path: '/playground', icon: Play, keywords: ['chat', 'test'] },
    ],
  },
  {
    labelKey: 'nav.groups.runtime',
    items: [
      { labelKey: 'nav.executions', path: '/executions', icon: CircleGauge, keywords: ['trace', 'run'] },
      { labelKey: 'nav.approvals', path: '/approvals', icon: ShieldCheck, badge: '3', keywords: ['review', 'todo'] },
      { labelKey: 'nav.datasets', path: '/datasets', icon: Database, keywords: ['cases', 'test'] },
      { labelKey: 'nav.evaluations', path: '/evaluations', icon: FlaskConical, keywords: ['report', 'quality'] },
    ],
  },
  {
    labelKey: 'nav.groups.resources',
    items: [
      { labelKey: 'nav.models', path: '/models', icon: BrainCircuit, keywords: ['llm', 'provider'] },
      { labelKey: 'nav.tools', path: '/tools', icon: Wrench, keywords: ['connector', 'mcp'] },
      { labelKey: 'nav.skills', path: '/skills', icon: Sparkles, keywords: ['skill', 'agent', 'capability'] },
      { labelKey: 'nav.knowledge', path: '/knowledge', icon: Library, keywords: ['rag', 'lightrag'] },
      { labelKey: 'nav.memory', path: '/memory', icon: MemoryStick, keywords: ['mem0'] },
    ],
  },
  {
    labelKey: 'nav.groups.organization',
    items: [
      { labelKey: 'nav.organization', path: '/organization', icon: UsersRound, keywords: ['department', 'user'] },
      { labelKey: 'nav.roles', path: '/roles', icon: KeyRound, keywords: ['rbac', 'permission'] },
    ],
  },
]

export const navigationItems = navigationGroups.flatMap((group) => group.items)

export const notifications: NotificationItem[] = [
  {
    id: 'approval-refund',
    titleKey: 'notifications.refund.title',
    descriptionKey: 'notifications.refund.description',
    path: '/approvals',
    tone: 'warning',
    timeKey: 'notifications.time.minutes',
  },
  {
    id: 'evaluation-complete',
    titleKey: 'notifications.evaluation.title',
    descriptionKey: 'notifications.evaluation.description',
    path: '/evaluations',
    tone: 'success',
    timeKey: 'notifications.time.hour',
  },
  {
    id: 'execution-failed',
    titleKey: 'notifications.execution.title',
    descriptionKey: 'notifications.execution.description',
    path: '/executions',
    tone: 'primary',
    timeKey: 'notifications.time.hours',
  },
]
