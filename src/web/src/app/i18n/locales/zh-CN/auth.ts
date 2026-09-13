const translations = {
  "brand": "企业级智能体工作流",
  "companyName": "公司名称",
  "username": "用户名",
  "displayName": "管理员姓名",
  "password": "密码",
  "newPassword": "新密码",
  "confirmPassword": "确认新密码",
  "passwordHint": "密码长度为 12–128 个字符，系统不会自动去除首尾空格。",
  "passwordMismatch": "两次输入的密码不一致",
  "setup": {
    "eyebrow": "首次初始化",
    "title": "创建企业工作台",
    "description": "设置当前部署唯一的公司和公司管理员。初始化完成后不可再次执行。",
    "submit": "初始化并进入工作台"
  },
  "login": {
    "eyebrow": "企业登录",
    "title": "登录 Agentx",
    "description": "使用公司管理员为你创建的账号登录。",
    "submit": "登录"
  },
  "change": {
    "eyebrow": "账号安全",
    "title": "设置正式密码",
    "description": "该账号正在使用临时密码，继续前必须设置正式密码。",
    "submit": "更新密码并登录"
  },
  "forbidden": {
    "title": "无权访问",
    "description": "你的角色和部门数据范围不允许访问此内容。"
  }
} as const

export default translations
