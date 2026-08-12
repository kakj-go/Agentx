const translations = {
  "title": "部门与用户",
  "description": "管理公司内部门树、用户归属和账号状态。",
  "departments": "部门",
  "users": "用户",
  "userName": "用户名称",
  "department": "部门",
  "roles": "角色",
  "allDepartments": "全部部门",
  "addDepartment": "新建部门",
  "addChildDepartment": "在“{{name}}”下新建部门",
  "editDepartment": "编辑或移动部门",
  "parentDepartment": "上级部门",
  "createUser": "创建用户",
  "editUser": "编辑用户",
  "initialPassword": "初始密码",
  "initialPasswordHint": "新用户的初始密码固定为 123456，首次登录后必须设置正式密码。",
  "expand": "展开部门",
  "collapse": "收起部门",
  "disable": "停用",
  "deleted": "删除成功",
  "dataScopes": {
    "company": "全公司",
    "department_tree": "本部门及下级部门",
    "own": "仅本人"
  }
} as const

export default translations
