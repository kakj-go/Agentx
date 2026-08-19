const translations = {
  loading: '正在加载 Trace…', unavailable: 'Trace 暂时不可用', delayed: 'Trace 摄取仍在追赶，当前展示已到达的 Span。', degraded: 'Trace 存在摄取冲突，当前结果可能不完整。',
  search: '搜索 Span、ID 或错误码', filterKind: '筛选 Span 类型', allKinds: '全部类型', errorsOnly: '仅异常', expandAll: '全部展开', zoom: '缩放', loaded: '已加载 {{loaded}} / {{total}}', loadMore: '加载更多', noMatches: '没有匹配的 Span',
  tree: 'Trace 层级瀑布', hierarchy: '调用层级', status: '状态', duration: '耗时', timeline: '时间轴', expand: '展开', collapse: '折叠', selectSpan: '选择一个 Span 查看详情', loadingDetail: '正在加载 Span 详情…',
  overview: '概览', input: '输入', output: '输出', events: '事件', raw: '原始数据', noInput: '该 Span 没有输入内容。', noOutput: '该 Span 没有输出内容。', startedAt: '开始时间', tokens: '输入 / 输出 Token', cost: '成本', attributes: '运行属性',
  kinds: { execution: '执行', node: '节点', attempt: '尝试', agent_run: '智能体运行', agent_iteration: '智能体迭代', runtime_call: 'Runtime 调用', sandbox: '沙箱', wait: '等待 / 审批' },
} as const
export default translations
