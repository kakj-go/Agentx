const translations = {
  "readAll": "全部标为已读",
  "notificationsDescription": "查看审批、执行、发布和评测产生的业务消息。",
  "noNotificationsDescription": "新的业务事件将投影到此消息中心。",
  "execution": {"title": "执行出现工具错误", "description": "合同审查工作流的文件解析工具调用失败。"},
  "approvalReassigned": {"title": "有一项审批已指派给你", "body": "请进入审批详情领取并处理该任务。"},
  "resourceGrantRequested": {"title": "有新的资源授权申请", "body": "请进入资源授权审批详情处理所属部门的会签项。"},
  "resourceGrantApproved": {"title": "资源授权申请已通过", "body": "资源现在可以在工作流画布中选择。"},
  "resourceGrantRejected": {"title": "资源授权申请已拒绝", "body": "请查看审批详情并按需重新申请。"},
  "resourceGrantStale": {"title": "资源授权申请已失效", "body": "资源依赖、工作流或申请人权限已发生变化，请重新申请。"},
  "resourceGrantCancelled": {"title": "资源授权申请已取消", "body": "该申请已取消，不会创建任何资源授权。"}
} as const

export default translations
