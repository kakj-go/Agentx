const translations = {
  title: '画布插件',
  description: '导入并管理工作流画布中的自定义节点、交互界面和执行逻辑。',
  import: '导入插件', template: '下载开发模板', search: '搜索插件', details: '详情',
  fields: { name: '名称', packageId: '包 ID', versions: '版本', nodes: '节点', updatedAt: '更新时间', status: '状态', source: '来源' },
  sources: { builtin: '内置', imported: '导入' }, enabled: '已启用', disabled: '已停用',
  importTitle: '导入画布插件', chooseFile: '选择 .agentx-plugin 文件', validating: '正在校验插件包…',
  confirmImport: '确认导入', imported: '插件已导入',
  enableAfterImport: '导入后立即启用', defaultAfterImport: '设为新建节点的默认版本',
  versions: '版本', drafts: '草稿', deployments: '应用部署', executionArtifacts: '冻结执行制品', usage: '使用情况', development: '开发说明', audit: '审计记录',
  enable: '启用', disable: '停用', setDefault: '设为默认', defaultVersion: '默认版本',
  downloadVersion: '下载插件版本', deleteVersion: '删除插件版本',
  uninstall: '卸载插件', referenceCount: '引用总数',
  confirmDelete: '确认删除该版本？存在引用时服务端会拒绝。', confirmUninstall: '确认卸载该插件？',
  templateHint: '模板包含 SDK、AGENTS.md、字段级协议和构建打包命令。', builtinHint: '该内置包使用与外部插件相同的公开执行协议，由 Agentx 随版本统一发布。', loadFailed: '无法加载画布插件。',
} as const
export default translations
