import * as React from 'react'
import { Component } from 'react'

export type PluginPanelComponentProps = {
  parameters: Record<string, unknown>
  readOnly: boolean
  fieldErrors: Record<string, string>
  providerOptions: Record<string, unknown[]>
  referenceCatalog?: unknown
  resources: Record<string, unknown>
  updateParameters: (patch: Record<string, unknown>) => void
}
export type PluginUi = {
  Panel: React.ComponentType<PluginPanelComponentProps>
  Canvas?: React.ComponentType<{ parameters: Record<string, unknown> }>
  Result?: React.ComponentType<{ value: unknown }>
  traceRenderers?: Record<string, React.ComponentType<{ value: unknown }>>
}
type PluginModule = { createUi: (host: unknown) => PluginUi }
export type PluginUiHostContext = {
  locale: string
  theme: 'light' | 'dark'
  portalRoot: HTMLElement
  assets: Record<string, string>
  design?: {
    resolveDefinition(configuration: Record<string, unknown>, upstreamContracts?: Record<string, unknown>, signal?: AbortSignal): Promise<unknown>
    invokeProvider(provider: string, input?: { search?: string; limit?: number; cursor?: string; parameters?: Record<string, unknown> }, signal?: AbortSignal): Promise<{ items: unknown[]; nextCursor?: string | null }>
  }
}
const modules = new Map<string, Promise<PluginModule>>()
const stylesheets = new Map<string, { element: HTMLStyleElement; references: number }>()
const MAX_CACHED_PLUGIN_MODULES = 64

export function loadPluginUi(source: string, digest: string) {
  let promise = modules.get(digest)
  if (promise) {
    modules.delete(digest)
    modules.set(digest, promise)
  }
  if (!promise) {
    while (modules.size >= MAX_CACHED_PLUGIN_MODULES) modules.delete(modules.keys().next().value!)
    const encoded = btoa(unescape(encodeURIComponent(source)))
    promise = (import(/* @vite-ignore */ `data:text/javascript;base64,${encoded}`) as Promise<PluginModule>)
      .catch((error) => { modules.delete(digest); throw error })
    modules.set(digest, promise)
  }
  return promise
}

export function pluginUiLoaderDiagnostics() {
  return { cachedModules: modules.size, installedStylesheets: stylesheets.size }
}

export function installPluginStyles(styles: string | null | undefined, digest: string) {
  if (!styles) return () => undefined
  let stylesheet = stylesheets.get(digest)
  if (!stylesheet) {
    const element = document.createElement('style')
    element.dataset.agentxPlugin = digest
    element.textContent = styles
    document.head.append(element)
    stylesheet = { element, references: 0 }
    stylesheets.set(digest, stylesheet)
  }
  stylesheet.references += 1
  return () => {
    const current = stylesheets.get(digest)
    if (!current) return
    current.references -= 1
    if (current.references <= 0) {
      current.element.remove()
      stylesheets.delete(digest)
    }
  }
}

export class PluginUiBoundary extends Component<{ children: React.ReactNode; fallback?: React.ReactNode }, { failed: boolean }> {
  state = { failed: false }
  static getDerivedStateFromError() { return { failed: true } }
  componentDidUpdate(previous: { children: React.ReactNode }) { if (previous.children !== this.props.children && this.state.failed) this.setState({ failed: false }) }
  render() { return this.state.failed ? (this.props.fallback ?? React.createElement('div', { className: 'rounded-md border border-danger/30 bg-danger/5 p-3 text-xs text-danger', role: 'alert' }, 'Plugin UI failed to render.')) : this.props.children }
}
