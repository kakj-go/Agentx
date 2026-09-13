import type { CreatePluginUi } from '@agentx/plugin-ui'

export const createUi: CreatePluginUi = (host) => ({
  Panel: function Panel({ parameters, readOnly, fieldErrors, updateParameters }) {
    const [search, setSearch] = host.React.useState('')
    const [labels, setLabels] = host.React.useState<unknown[]>([])
    const pending = host.React.useRef<AbortController>()
    const loadLabels = async () => {
      pending.current?.abort()
      const controller = new AbortController()
      pending.current = controller
      try {
        const result = await host.design!.invokeProvider('labels', { search, limit: 2, parameters }, controller.signal)
        if (!controller.signal.aborted) setLabels(result.items)
      } catch (error) {
        if (!controller.signal.aborted) throw error
      }
    }
    return host.React.createElement('div', { className: 'space-y-3' },
      host.React.createElement(host.components.Field, { label: 'Label', error: fieldErrors.label },
        host.React.createElement(host.components.Input, {
          value: String(parameters.label ?? ''), disabled: readOnly,
          onChange: (event: { target: { value: string } }) => updateParameters({ label: event.target.value }),
        }),
      ),
      host.React.createElement(host.components.Field, { label: 'Metadata output' },
        host.React.createElement(host.components.Input, {
          type: 'checkbox', checked: parameters.includeMetadata === true, disabled: readOnly,
          onChange: (event: { target: { checked: boolean } }) => updateParameters({ includeMetadata: event.target.checked }),
        }),
      ),
      host.React.createElement(host.components.Field, { label: 'Large trace artifact' },
        host.React.createElement(host.components.Input, {
          type: 'checkbox', checked: parameters.largeTrace === true, disabled: readOnly,
          onChange: (event: { target: { checked: boolean } }) => updateParameters({ largeTrace: event.target.checked }),
        }),
      ),
      host.React.createElement(host.components.Field, { label: 'Search provider' },
        host.React.createElement('div', { className: 'flex gap-2' },
          host.React.createElement(host.components.Input, { value: search, disabled: readOnly, onChange: (event: { target: { value: string } }) => setSearch(event.target.value) }),
          host.React.createElement(host.components.Button, { type: 'button', disabled: readOnly || !host.design, onClick: loadLabels }, 'Search'),
        ),
      ),
      host.React.createElement('div', { 'data-testid': 'plugin-provider-results', className: 'text-xs text-muted-foreground' }, labels.map((item) => String((item as { label?: unknown }).label ?? '')).join(', ')),
    )
  },
  Canvas: ({ parameters }) => host.React.createElement('span', { className: 'flex items-center gap-1 truncate text-muted-foreground' }, host.React.createElement('img', { src: host.assets.logo, alt: '', className: 'size-3' }), `Label: ${String(parameters.label ?? '—')}`),
  Result: ({ value }) => host.React.createElement('div', { className: 'rounded-md border border-border p-3 text-xs' }, `Plugin result: ${JSON.stringify(value)}`),
  traceRenderers: {
    summary: ({ value }) => host.React.createElement('div', { className: 'rounded-md border border-border p-3 text-xs' }, `Mapped items: ${String((value as { mapped?: number })?.mapped ?? 0)}`),
    details: ({ value }) => host.React.createElement('div', { className: 'rounded-md border border-border p-3 text-xs' }, `Payload bytes: ${String((value as { payload?: string })?.payload?.length ?? 0)}`),
    table: ({ value }) => {
      const table = value as { columns?: string[]; rows?: unknown[][] }
      return host.React.createElement('div', { className: 'overflow-auto rounded-md border border-border' },
        host.React.createElement('table', { className: 'w-full text-left text-xs' },
          host.React.createElement('thead', null, host.React.createElement('tr', null, (table.columns ?? []).map((column) => host.React.createElement('th', { className: 'border-b border-border px-2 py-1', key: column }, column)))),
          host.React.createElement('tbody', null, (table.rows ?? []).map((row, rowIndex) => host.React.createElement('tr', { key: rowIndex }, row.map((cell, columnIndex) => host.React.createElement('td', { className: 'px-2 py-1', key: columnIndex }, String(cell)))))),
        ),
      )
    },
  },
})
