import {
  AppWindow,
  Blocks,
  BrainCircuit,
  CircleGauge,
  Database,
  FlaskConical,
  KeyRound,
  Library,
  LockKeyhole,
  MemoryStick,
  Play,
  Shield,
  ShieldCheck,
  Sparkles,
  UsersRound,
  ServerCog,
  Box,
  Bell,
  RadioTower,
} from 'lucide-react'

import type { NavigationGroup } from '../shared/types/app'

export const navigationGroups: NavigationGroup[] = [
  {
    labelKey: 'nav.groups.workspace',
    items: [
      { labelKey: 'nav.dashboard', path: '/', icon: CircleGauge, keywords: ['overview', 'home'] },
      { labelKey: 'nav.workflows', path: '/workflows', icon: Blocks, keywords: ['agent', 'flow'], requiredPermission: 'workflow:view' },
      { labelKey: 'nav.applications', path: '/applications', icon: AppWindow, keywords: ['deployment', 'api'], requiredPermission: 'application:view' },
      { labelKey: 'nav.playground', path: '/playground', icon: Play, keywords: ['chat', 'test'], requiredPermission: 'application:invoke' },
    ],
  },
  {
    labelKey: 'nav.groups.runtime',
    items: [
      { labelKey: 'nav.executions', path: '/executions', icon: CircleGauge, keywords: ['trace', 'run'], requiredPermission: 'execution:view' },
      { labelKey: 'nav.approvals', path: '/approvals', icon: ShieldCheck, keywords: ['review', 'todo'], requiredPermission: 'approval:view' },
      { labelKey: 'nav.notifications', path: '/notifications', icon: Bell, keywords: ['inbox', 'message'], requiredPermission: 'notification:view' },
      { labelKey: 'nav.datasets', path: '/datasets', icon: Database, keywords: ['cases', 'test'], requiredPermission: 'dataset:view' },
      { labelKey: 'nav.evaluations', path: '/evaluations', icon: FlaskConical, keywords: ['report', 'quality'], requiredPermission: 'evaluation:view' },
      { labelKey: 'nav.runtime', path: '/runtime', icon: RadioTower, keywords: ['worker', 'queue'], requiredPermission: 'runtime:view' },
    ],
  },
  {
    labelKey: 'nav.groups.resources',
    items: [
      { labelKey: 'nav.credentials', path: '/credentials', icon: LockKeyhole, keywords: ['secret', 'key'], requiredPermission: 'credential:view' },
      { labelKey: 'nav.models', path: '/models', icon: BrainCircuit, keywords: ['llm', 'provider'], requiredPermission: 'model:view' },
      { labelKey: 'nav.mcp', path: '/mcp', icon: ServerCog, keywords: ['connector', 'server', 'tool'], requiredPermission: 'mcp:view' },
      { labelKey: 'nav.skills', path: '/skills', icon: Sparkles, keywords: ['skill', 'agent', 'capability'], requiredPermission: 'skill:view' },
      { labelKey: 'nav.knowledge', path: '/knowledge', icon: Library, keywords: ['rag', 'lightrag'], requiredPermission: 'knowledge:view' },
      { labelKey: 'nav.memory', path: '/memory', icon: MemoryStick, keywords: ['mem0'], requiredPermission: 'memory:view' },
      { labelKey: 'nav.sandboxProfiles', path: '/sandbox-profiles', icon: Box, keywords: ['sandbox', 'profile', 'runner'], requiredPermission: 'sandbox:view' },
      { labelKey: 'nav.resourceGrants', path: '/resource-grants', icon: Shield, keywords: ['grant', 'permission', 'resource'], requiredPermission: 'resource:grant' },
    ],
  },
  {
    labelKey: 'nav.groups.organization',
    items: [
      { labelKey: 'nav.organization', path: '/organization', icon: UsersRound, keywords: ['department', 'user'], requiredPermission: 'user:view' },
      { labelKey: 'nav.roles', path: '/roles', icon: KeyRound, keywords: ['rbac', 'permission'], requiredPermission: 'role:view' },
    ],
  },
]

export const navigationItems = navigationGroups.flatMap((group) => group.items)
