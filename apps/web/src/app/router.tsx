/* oxlint-disable react/only-export-components */
import { lazy, Suspense } from 'react'
import { createBrowserRouter } from 'react-router-dom'

import { EnterpriseLayout } from '../layouts/enterprise-workbench/enterprise-layout'
import { DashboardPage } from '../pages/dashboard-page'
import { PlaceholderPage } from '../pages/placeholder-page'

const placeholder = (title: string, description: string) => (
  <PlaceholderPage title={title} description={description} />
)

const WorkflowCanvas = lazy(() =>
  import('../features/workflow-designer/workflow-canvas').then((module) => ({
    default: module.WorkflowCanvas,
  })),
)

const workflowCanvas = (
  <Suspense fallback={<div className="grid h-full place-items-center text-sm text-muted-foreground">正在加载画布…</div>}>
    <WorkflowCanvas />
  </Suspense>
)

export const router = createBrowserRouter([
  {
    element: <EnterpriseLayout />,
    children: [
      { index: true, element: <DashboardPage /> },
      { path: 'workflows', element: workflowCanvas },
      { path: 'applications', element: placeholder('应用', '管理已发布的 Workflow 应用与版本。') },
      { path: 'playground', element: placeholder('Playground', '通过正式应用 API 调试会话和消息。') },
      { path: 'executions', element: placeholder('执行记录', '查询 Workflow Execution、Trace 和 Checkpoint。') },
      { path: 'approvals', element: placeholder('待审批', '处理 Workflow 审批节点产生的任务。') },
      { path: 'datasets', element: placeholder('测试集', '管理测试用例及不可变 Dataset Version。') },
      { path: 'evaluations', element: placeholder('评测报告', '比较 Workflow Version 的质量、成本与耗时。') },
      { path: 'models', element: placeholder('模型服务', '管理模型连接、别名、价格和 Workflow 授权。') },
      { path: 'tools', element: placeholder('工具与连接', '管理 HTTP、OpenAPI、MCP 和沙箱工具。') },
      { path: 'knowledge', element: placeholder('知识库', '管理 LightRAG 连接和知识库权限。') },
      { path: 'memory', element: placeholder('Memory', '管理 Mem0 连接、Namespace 和读写权限。') },
      { path: 'organization', element: placeholder('部门与用户', '管理租户内部门树和用户。') },
      { path: 'roles', element: placeholder('角色权限', '管理操作权限、数据范围和资源授权。') },
    ],
  },
])
