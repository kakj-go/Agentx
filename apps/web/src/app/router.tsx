/* oxlint-disable react/only-export-components */
import { lazy, Suspense, type ReactNode } from 'react'
import { createBrowserRouter } from 'react-router-dom'

import { DashboardPage } from '../features/dashboard/dashboard-page'
import { NotFoundPage } from '../features/not-found/not-found-page'
import { EnterpriseLayout } from '../layouts/enterprise-workbench/enterprise-layout'

const ApplicationsPage = lazy(() => import('../features/applications/applications-page').then((module) => ({ default: module.ApplicationsPage })))
const ApprovalsPage = lazy(() => import('../features/approvals/approvals-page').then((module) => ({ default: module.ApprovalsPage })))
const DatasetsPage = lazy(() => import('../features/datasets/datasets-page').then((module) => ({ default: module.DatasetsPage })))
const EvaluationsPage = lazy(() => import('../features/evaluations/evaluations-page').then((module) => ({ default: module.EvaluationsPage })))
const ExecutionsPage = lazy(() => import('../features/executions/executions-page').then((module) => ({ default: module.ExecutionsPage })))
const KnowledgePage = lazy(() => import('../features/knowledge/knowledge-page').then((module) => ({ default: module.KnowledgePage })))
const MemoryPage = lazy(() => import('../features/memory/memory-page').then((module) => ({ default: module.MemoryPage })))
const ModelsPage = lazy(() => import('../features/models/models-page').then((module) => ({ default: module.ModelsPage })))
const OrganizationPage = lazy(() => import('../features/organization/organization-page').then((module) => ({ default: module.OrganizationPage })))
const PlaygroundPage = lazy(() => import('../features/playground/playground-page').then((module) => ({ default: module.PlaygroundPage })))
const RolesPage = lazy(() => import('../features/roles/roles-page').then((module) => ({ default: module.RolesPage })))
const SkillsPage = lazy(() => import('../features/skills/skills-page').then((module) => ({ default: module.SkillsPage })))
const ToolsPage = lazy(() => import('../features/tools/tools-page').then((module) => ({ default: module.ToolsPage })))
const WorkflowsPage = lazy(() => import('../features/workflows/workflows-page').then((module) => ({ default: module.WorkflowsPage })))
const WorkflowCanvas = lazy(() => import('../features/workflow-designer/workflow-canvas').then((module) => ({ default: module.WorkflowCanvas })))

function deferred(element: ReactNode) {
  return <Suspense fallback={<div className="grid h-full min-h-72 place-items-center text-sm text-muted-foreground">Agentx…</div>}>{element}</Suspense>
}

export const router = createBrowserRouter([
  {
    element: <EnterpriseLayout />,
    children: [
      { index: true, element: <DashboardPage /> },
      { path: 'workflows', element: deferred(<WorkflowsPage />) },
      { path: 'workflows/:workflowId/editor', element: deferred(<WorkflowCanvas />) },
      { path: 'applications', element: deferred(<ApplicationsPage />) },
      { path: 'playground', element: deferred(<PlaygroundPage />) },
      { path: 'executions', element: deferred(<ExecutionsPage />) },
      { path: 'approvals', element: deferred(<ApprovalsPage />) },
      { path: 'datasets', element: deferred(<DatasetsPage />) },
      { path: 'evaluations', element: deferred(<EvaluationsPage />) },
      { path: 'models', element: deferred(<ModelsPage />) },
      { path: 'tools', element: deferred(<ToolsPage />) },
      { path: 'skills', element: deferred(<SkillsPage />) },
      { path: 'knowledge', element: deferred(<KnowledgePage />) },
      { path: 'memory', element: deferred(<MemoryPage />) },
      { path: 'organization', element: deferred(<OrganizationPage />) },
      { path: 'roles', element: deferred(<RolesPage />) },
      { path: '*', element: <NotFoundPage /> },
    ],
  },
])
