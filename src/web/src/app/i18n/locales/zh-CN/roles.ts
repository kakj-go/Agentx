const translations = {
  "title": "角色权限",
  "description": "管理操作权限、数据范围和资源授权。",
  "create": "新建角色",
  "search": "搜索角色",
  "dataScope": "数据范围",
  "permissions": "权限摘要",
  "code": "角色标识",
  "deleted": "删除成功",
  "names": {
    "company_admin": "公司管理员",
    "department_admin": "部门管理员",
    "member": "普通成员"
  },
  "permissionLabels": {
    "company": {
      "view": "查看公司",
      "manage": "管理公司"
    },
    "department": {
      "view": "查看部门",
      "manage": "管理部门"
    },
    "user": {
      "view": "查看用户",
      "create": "创建用户",
      "update": "编辑用户",
      "disable": "停用用户"
    },
    "role": {
      "view": "查看角色",
      "manage": "管理角色",
      "assign": "分配角色"
    },
    "workflow": {
      "view": "查看工作流",
      "create": "创建工作流",
      "edit": "编辑工作流",
      "archive": "归档工作流",
      "publish": "发布工作流",
      "manage_member": "管理工作流成员",
      "manage_permission": "管理工作流权限"
    },
    "credential": {
      "view": "查看凭证",
      "manage": "管理凭证"
    },
    "model": {
      "view": "查看模型",
      "manage": "管理模型"
    },
    "mcp": {
      "view": "查看 MCP",
      "manage": "管理 MCP",
      "discover": "发现 MCP 工具",
      "debug": "调试 MCP 工具"
    },
    "skill": {
      "view": "查看技能",
      "manage": "管理技能"
    },
    "knowledge": {
      "view": "查看知识库",
      "manage": "管理知识库"
    },
    "memory": {
      "view": "查看记忆",
      "manage": "管理记忆"
    },
    "sandbox": {
      "view": "查看沙箱配置",
      "manage": "管理沙箱配置"
    },
    "resource": {
      "grant": "授予资源"
    },
    "application": {
      "view": "查看应用",
      "manage": "管理应用",
      "manage_key": "管理 API Key",
      "invoke": "调用应用"
    },
    "dataset": {
      "view": "查看测试集",
      "manage": "管理测试集"
    },
    "evaluation": {
      "view": "查看评测",
      "manage": "管理评测"
    },
    "evaluation_profile": {
      "view": "查看评测方案",
      "manage": "管理评测方案"
    },
    "approval": {
      "view": "查看审批",
      "act": "处理审批",
      "manage": "管理审批"
    },
    "notification": {
      "view": "查看消息"
    },
    "execution": {
      "view": "查看执行",
      "cancel": "取消执行"
    },
    "trace": {
      "view": "查看 Trace"
    },
    "runtime": {
      "view": "查看运行状态"
    }
  }
} as const

export default translations
