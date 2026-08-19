const translations = {
  loading: 'Loading trace…', unavailable: 'Trace is temporarily unavailable', delayed: 'Trace ingestion is catching up. Showing spans received so far.', degraded: 'Trace ingestion conflicts were detected; results may be incomplete.',
  search: 'Search spans, IDs, or errors', filterKind: 'Filter span kind', allKinds: 'All kinds', errorsOnly: 'Errors only', expandAll: 'Expand all', zoom: 'Zoom', loaded: '{{loaded}} / {{total}} loaded', loadMore: 'Load more', noMatches: 'No matching spans',
  tree: 'Trace hierarchy waterfall', hierarchy: 'Call hierarchy', status: 'Status', duration: 'Duration', timeline: 'Timeline', expand: 'Expand', collapse: 'Collapse', selectSpan: 'Select a span to inspect details', loadingDetail: 'Loading span details…',
  overview: 'Overview', input: 'Input', output: 'Output', events: 'Events', raw: 'Raw', noInput: 'This span has no input content.', noOutput: 'This span has no output content.', startedAt: 'Started at', tokens: 'Input / output tokens', cost: 'Cost', attributes: 'Runtime attributes',
  kinds: { execution: 'Execution', node: 'Node', attempt: 'Attempt', agent_run: 'Agent run', agent_iteration: 'Agent iteration', runtime_call: 'Runtime call', sandbox: 'Sandbox', wait: 'Wait / approval' },
} as const
export default translations
