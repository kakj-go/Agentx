/* oxlint-disable react/only-export-components */
import { lazy, Suspense, type ReactNode } from 'react'
import { createBrowserRouter } from 'react-router-dom'

import { DashboardPage } from '../features/dashboard/dashboard-page'
import { NotFoundPage } from '../features/not-found/not-found-page'
import { EnterpriseLayout } from '../layouts/enterprise-workbench/enterprise-layout'
import { Protected, PublicRoute, RequirePermission } from './auth-guards'
import { SetupPage } from '../features/auth/setup-page'
import { LoginPage } from '../features/auth/login-page'
import { ChangePasswordPage } from '../features/auth/change-password-page'
import { ForbiddenPage } from '../features/auth/forbidden-page'

const ApplicationsPage = lazy(() => import('../features/applications/applications-page').then((module) => ({ default: module.ApplicationsPage })))
const ApprovalsPage = lazy(() => import('../features/approvals/approvals-page').then((module) => ({ default: module.ApprovalsPage })))
const DatasetsPage = lazy(() => import('../features/datasets/datasets-page').then((module) => ({ default: module.DatasetsPage })))
const EvaluationsPage = lazy(() => import('../features/evaluations/evaluations-page').then((module) => ({ default: module.EvaluationsPage })))
const ExecutionsPage = lazy(() => import('../features/executions/executions-page').then((module) => ({ default: module.ExecutionsPage })))
const KnowledgePage = lazy(() => import('../features/knowledge/knowledge-page').then((module) => ({ default: module.KnowledgePage })))
const KnowledgeDetailPage = lazy(() => import('../features/knowledge/knowledge-detail-page').then((module) => ({ default: module.KnowledgeDetailPage })))
const MemoryPage = lazy(() => import('../features/memory/memory-page').then((module) => ({ default: module.MemoryPage })))
const MemoryDetailPage = lazy(() => import('../features/memory/memory-detail-page').then((module) => ({ default: module.MemoryDetailPage })))
const ModelsPage = lazy(() => import('../features/models/models-page').then((module) => ({ default: module.ModelsPage })))
const ModelDetailPage = lazy(() => import('../features/models/model-detail-page').then((module) => ({ default: module.ModelDetailPage })))
const OrganizationPage = lazy(() => import('../features/organization/organization-page').then((module) => ({ default: module.OrganizationPage })))
const PlaygroundPage = lazy(() => import('../features/playground/playground-page').then((module) => ({ default: module.PlaygroundPage })))
const RolesPage = lazy(() => import('../features/roles/roles-page').then((module) => ({ default: module.RolesPage })))
const SkillsPage = lazy(() => import('../features/skills/skills-page').then((module) => ({ default: module.SkillsPage })))
const SkillDetailPage = lazy(() => import('../features/skills/skill-detail-page').then((module) => ({ default: module.SkillDetailPage })))
const McpServersPage = lazy(() => import('../features/mcp/mcp-servers-page').then((module) => ({ default: module.McpServersPage })))
const McpServerDetailPage = lazy(() => import('../features/mcp/mcp-server-detail-page').then((module) => ({ default: module.McpServerDetailPage })))
const WorkflowsPage = lazy(() => import('../features/workflows/workflows-page').then((module) => ({ default: module.WorkflowsPage })))
const WorkflowDetailPage = lazy(() => import('../features/workflows/workflow-detail-page').then((module) => ({ default: module.WorkflowDetailPage })))
const WorkflowCanvas = lazy(() => import('../features/workflow-designer/workflow-canvas').then((module) => ({ default: module.WorkflowCanvas })))
const CredentialsPage = lazy(() => import('../features/credentials/credentials-page').then((module) => ({ default: module.CredentialsPage })))
const CredentialDetailPage = lazy(() => import('../features/credentials/credential-detail-page').then((module) => ({ default: module.CredentialDetailPage })))
const ResourceGrantsPage = lazy(() => import('../features/resource-grants/resource-grants-page').then((module) => ({ default: module.ResourceGrantsPage })))

function deferred(element: ReactNode) {
  return <Suspense fallback={<div className="grid h-full min-h-72 place-items-center text-sm text-muted-foreground">Agentx…</div>}>{element}</Suspense>
}

export const router = createBrowserRouter([
  { path: 'setup', element: <PublicRoute kind="setup"><SetupPage /></PublicRoute> },
  { path: 'login', element: <PublicRoute kind="login"><LoginPage /></PublicRoute> },
  { path: 'change-password', element: <PublicRoute kind="change-password"><ChangePasswordPage /></PublicRoute> },
  { path: '403', element: <Protected><ForbiddenPage /></Protected> },
  {
    element: <Protected><EnterpriseLayout /></Protected>,
    children: [
      { index: true, element: <DashboardPage /> },
      { path: 'workflows', element: deferred(<RequirePermission permission="workflow:view"><WorkflowsPage /></RequirePermission>) },
      { path: 'workflows/:workflowId', element: deferred(<RequirePermission permission="workflow:view"><WorkflowDetailPage /></RequirePermission>) },
      { path: 'workflows/:workflowId/editor', element: deferred(<RequirePermission permission="workflow:edit"><WorkflowCanvas /></RequirePermission>) },
      { path: 'applications', element: deferred(<ApplicationsPage />) },
      { path: 'playground', element: deferred(<PlaygroundPage />) },
      { path: 'executions', element: deferred(<ExecutionsPage />) },
      { path: 'approvals', element: deferred(<ApprovalsPage />) },
      { path: 'datasets', element: deferred(<DatasetsPage />) },
      { path: 'evaluations', element: deferred(<EvaluationsPage />) },
      { path: 'credentials', element: deferred(<RequirePermission permission="credential:view"><CredentialsPage /></RequirePermission>) },
      { path: 'credentials/:id', element: deferred(<RequirePermission permission="credential:view"><CredentialDetailPage /></RequirePermission>) },
      { path: 'models', element: deferred(<RequirePermission permission="model:view"><ModelsPage /></RequirePermission>) },
      { path: 'models/:id', element: deferred(<RequirePermission permission="model:view"><ModelDetailPage /></RequirePermission>) },
      { path: 'mcp', element: deferred(<RequirePermission permission="mcp:view"><McpServersPage /></RequirePermission>) },
      { path: 'mcp/:id', element: deferred(<RequirePermission permission="mcp:view"><McpServerDetailPage /></RequirePermission>) },
      { path: 'skills', element: deferred(<RequirePermission permission="skill:view"><SkillsPage /></RequirePermission>) },
      { path: 'skills/:id', element: deferred(<RequirePermission permission="skill:view"><SkillDetailPage /></RequirePermission>) },
      { path: 'knowledge', element: deferred(<RequirePermission permission="knowledge:view"><KnowledgePage /></RequirePermission>) },
      { path: 'knowledge/:id', element: deferred(<RequirePermission permission="knowledge:view"><KnowledgeDetailPage /></RequirePermission>) },
      { path: 'memory', element: deferred(<RequirePermission permission="memory:view"><MemoryPage /></RequirePermission>) },
      { path: 'memory/:id', element: deferred(<RequirePermission permission="memory:view"><MemoryDetailPage /></RequirePermission>) },
      { path: 'resource-grants', element: deferred(<RequirePermission permission="resource:grant"><ResourceGrantsPage /></RequirePermission>) },
      { path: 'organization', element: deferred(<RequirePermission permission="user:view"><OrganizationPage /></RequirePermission>) },
      { path: 'roles', element: deferred(<RequirePermission permission="role:view"><RolesPage /></RequirePermission>) },
      { path: '*', element: <NotFoundPage /> },
    ],
  },
])
