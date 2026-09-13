import { AsyncLocalStorage } from 'node:async_hooks'
import { createInterface } from 'node:readline'
import { pathToFileURL } from 'node:url'

const MAX_MESSAGE_BYTES = 8 * 1024 * 1024
const MAX_SPANS = 64
const MAX_SPAN_DEPTH = 16
const MAX_CONTENTS_PER_SPAN = 32
const MAX_EVENTS_PER_SPAN = 128
const MAX_ATTRIBUTES_PER_SPAN = 64
const active = new Map()
const hostResponses = new Map()
const spanContext = new AsyncLocalStorage()
let initialized = false
let inputClosed = false
let hostRequestSequence = 0
const protocolWrite = process.stdout.write.bind(process.stdout)

for (const level of ['log', 'info', 'debug', 'warn', 'error']) {
  console[level] = (...args) => process.stderr.write(`${args.map(String).join(' ')}\n`)
}

const lines = createInterface({ input: process.stdin, crlfDelay: Infinity })
lines.on('close', () => {
  inputClosed = true
  for (const pending of hostResponses.values()) pending.reject(new Error('Plugin host connection closed'))
  hostResponses.clear()
  if (active.size === 0) process.exitCode = 0
})
lines.on('line', (line) => void handleLine(line))

async function handleLine(line) {
  if (Buffer.byteLength(line) > MAX_MESSAGE_BYTES) return reply(null, undefined, rpcError(-32600, 'Message exceeds 8 MiB'))
  let message
  try { message = JSON.parse(line) } catch { return reply(null, undefined, rpcError(-32700, 'Parse error')) }
  if (message?.jsonrpc !== '2.0') return reply(message?.id ?? null, undefined, rpcError(-32600, 'Invalid Request'))
  if (!message.method && message.id !== undefined) {
    const pending = hostResponses.get(String(message.id))
    if (!pending) return
    hostResponses.delete(String(message.id))
    if (message.error) pending.reject(new HostCallError(message.error.code, message.error.message ?? 'Plugin host call failed'))
    else pending.resolve(message.result)
    return
  }
  if (!message.method) return reply(message?.id ?? null, undefined, rpcError(-32600, 'Invalid Request'))
  try {
    if (message.method === 'runner.initialize') {
      if (message.params?.protocolVersion !== 1 || message.params?.sdkApiVersion !== 1) {
        return reply(message.id, undefined, rpcError(-32010, 'Unsupported plugin protocol or SDK API version'))
      }
      initialized = true
      return reply(message.id, { protocolVersion: 1, sdkApiVersion: 1, nodeVersion: process.versions.node })
    }
    if (message.method === 'runner.shutdown') {
      reply(message.id, { stopped: true })
      process.exitCode = 0
      return lines.close()
    }
    if (message.method === 'invocation.cancel') {
      active.get(message.params?.invocationId)?.abort('cancelled')
      return reply(message.id, { cancelled: true })
    }
    if (!initialized) return reply(message.id, undefined, rpcError(-32011, 'Runner is not initialized'))
    if (message.method === 'node.resolveDefinition' || message.method === 'node.invokeProvider') return handleDesignOperation(message)
    if (message.method !== 'node.execute') return reply(message.id, undefined, rpcError(-32601, 'Method not found'))
    return handleExecution(message)
  } catch (error) {
    reply(message.id, undefined, rpcError(-32001, error instanceof Error ? error.message : String(error)))
  }
}

async function handleDesignOperation(request) {
  const params = request.params ?? {}
  const invocationId = `design:${request.id}`
  const controller = new AbortController()
  active.set(invocationId, controller)
  const timeout = Math.max(1, Number(params.deadlineMs ?? 10_000))
  const timer = setTimeout(() => controller.abort('deadline'), timeout)
  const scope = createInvocationScope()
  try {
    const module = await loadModule(params.runtimeSource, params.runtimeEntry, invocationId)
    if (request.method === 'node.resolveDefinition') {
      const result = typeof module.resolveDefinition === 'function'
        ? await module.resolveDefinition(params.configuration ?? {}, params.upstreamContracts ?? {}, { nodeType: String(params.nodeType ?? '') })
        : { status: 'complete', inputPorts: params.inputPorts ?? [], outputPorts: params.outputPorts ?? [], outputSchema: params.outputSchema ?? {}, outputPortSchemas: params.outputPortSchemas ?? {} }
      return reply(request.id, result)
    }
    const provider = module.providers?.[params.provider]
    if (typeof provider !== 'function') return reply(request.id, undefined, rpcError(-32601, 'Provider not found'))
    const context = {
      signal: controller.signal,
      http: (input) => hostCall(invocationId, 'host.http', { input }, controller.signal),
      model: (input) => hostCall(invocationId, 'host.model', { input }, controller.signal),
      credentials: { list: () => hostCall(invocationId, 'host.credentials.list', {}, controller.signal) },
      artifacts: { put: (input) => hostCall(invocationId, 'host.artifacts.put', { input }, controller.signal) },
    }
    return reply(request.id, await provider(params.input ?? {}, context))
  } catch (error) {
    const message = controller.signal.aborted
      ? `Invocation aborted: ${controller.signal.reason}`
      : error instanceof Error ? error.message : String(error)
    const code = controller.signal.aborted ? -32002 : error instanceof HostCallError ? error.code : -32001
    return reply(request.id, undefined, rpcError(code, message))
  } finally {
    scope.dispose()
    clearTimeout(timer)
    active.delete(invocationId)
  }
}

async function handleExecution(request) {
  const params = request.params ?? {}
  const invocationId = String(params.invocationId ?? '')
  if (!invocationId) return reply(request.id, undefined, rpcError(-32602, 'invocationId is required'))
  if (active.has(invocationId)) return reply(request.id, undefined, rpcError(-32012, 'Invocation is already active'))
  if (active.size > 0) return reply(request.id, undefined, rpcError(-32013, 'Runner already has an active invocation'))
  const controller = new AbortController()
  active.set(invocationId, controller)
  const timeout = Math.max(1, Date.parse(params.execution?.deadline ?? '') - Date.now())
  const timer = setTimeout(() => controller.abort('deadline'), timeout)
  const scope = createInvocationScope()
  try {
    const module = await loadModule(params.runtimeSource, params.runtimeEntry, invocationId)
    if (typeof module.execute !== 'function') throw new Error('Plugin runtime must export execute')
    const trace = []
    const context = {
      inputs: params.inputs ?? {}, parameters: params.parameters ?? {}, perItemParameters: params.perItemParameters ?? [],
      stringConversions: params.stringConversions ?? [], context: params.context ?? {}, execution: params.execution,
      signal: controller.signal,
      items: { fromJson: (values) => values.map((json) => ({ json })) },
      http: (input) => hostCall(invocationId, 'host.http', { input, parentIndex: spanContext.getStore() }, controller.signal),
      model: (input) => hostCall(invocationId, 'host.model', { input, parentIndex: spanContext.getStore() }, controller.signal),
      credentials: { list: () => hostCall(invocationId, 'host.credentials.list', { parentIndex: spanContext.getStore() }, controller.signal) },
      artifacts: { put: (input) => hostCall(invocationId, 'host.artifacts.put', { input, parentIndex: spanContext.getStore() }, controller.signal) },
      trace: { span: (name, body) => runSpan(invocationId, trace, name, body) },
    }
    const result = await module.execute(context)
    reply(request.id, { ...result, trace })
  } catch (error) {
    const message = controller.signal.aborted
      ? `Invocation aborted: ${controller.signal.reason}`
      : error instanceof Error ? error.message : String(error)
    const code = controller.signal.aborted ? -32002 : error instanceof HostCallError ? error.code : -32001
    reply(request.id, undefined, rpcError(code, message))
  } finally {
    scope.dispose()
    clearTimeout(timer)
    active.delete(invocationId)
    if (inputClosed && active.size === 0) process.exitCode = 0
  }
}

async function runSpan(invocationId, trace, name, body) {
  if (trace.length >= MAX_SPANS) throw new Error('PLUGIN_TRACE_BUDGET_EXCEEDED: span limit')
  const index = trace.length
  const parentIndex = spanContext.getStore()
  if (spanDepth(trace, parentIndex) >= MAX_SPAN_DEPTH) throw new Error('PLUGIN_TRACE_BUDGET_EXCEEDED: depth limit')
  const value = { name, status: 'running', attributes: {}, contents: [], events: [], startedAt: new Date().toISOString(), parentIndex }
  trace.push(value)
  notify('trace.event', { invocationId, index, parentIndex, phase: 'started', span: value })
  const span = {
    setAttribute: (key, item) => {
      if (!(key in value.attributes) && Object.keys(value.attributes).length >= MAX_ATTRIBUTES_PER_SPAN) throw new Error('PLUGIN_TRACE_BUDGET_EXCEEDED: attribute limit')
      value.attributes[key] = item
    },
    content: (item) => {
      if (value.contents.length >= MAX_CONTENTS_PER_SPAN) throw new Error('PLUGIN_TRACE_BUDGET_EXCEEDED: content limit')
      value.contents.push(item)
      notify('trace.event', { invocationId, index, parentIndex, phase: 'content', span: value, content: item })
    },
    event: (event, attributes = {}) => {
      if (value.events.length >= MAX_EVENTS_PER_SPAN) throw new Error('PLUGIN_TRACE_BUDGET_EXCEEDED: event limit')
      const item = { event, attributes, occurredAt: new Date().toISOString() }
      value.events.push(item)
      notify('trace.event', { invocationId, index, parentIndex, phase: 'event', span: value, event: item })
    },
  }
  try {
    const result = await spanContext.run(index, () => body(span))
    value.status = 'succeeded'
    return result
  } catch (error) {
    value.status = 'failed'
    value.error = String(error)
    throw error
  } finally {
    value.endedAt = new Date().toISOString()
    notify('trace.event', { invocationId, index, parentIndex, phase: 'finished', span: value })
  }
}

function hostCall(invocationId, method, params, signal) {
  if (signal.aborted) return Promise.reject(new Error(`Invocation aborted: ${signal.reason}`))
  const id = `host:${invocationId}:${++hostRequestSequence}`
  return new Promise((resolve, reject) => {
    const abort = () => {
      hostResponses.delete(id)
      reject(new Error(`Invocation aborted: ${signal.reason}`))
    }
    signal.addEventListener('abort', abort, { once: true })
    hostResponses.set(id, {
      resolve: (value) => { signal.removeEventListener('abort', abort); resolve(value) },
      reject: (error) => { signal.removeEventListener('abort', abort); reject(error) },
    })
    write({ jsonrpc: '2.0', id, method, params })
  })
}

async function loadModule(source, entry, cacheKey = Date.now().toString()) {
  if (source) return import(`data:text/javascript;base64,${Buffer.from(source).toString('base64')}#${encodeURIComponent(cacheKey)}`)
  if (entry) {
    const url = pathToFileURL(entry)
    url.searchParams.set('agentxInvocation', cacheKey)
    return import(url.href)
  }
  throw new Error('Plugin runtime source is missing')
}

function spanDepth(trace, parentIndex) {
  let depth = 0
  let current = parentIndex
  const seen = new Set()
  while (Number.isInteger(current) && !seen.has(current)) {
    seen.add(current)
    depth += 1
    current = trace[current]?.parentIndex
  }
  return depth
}

function createInvocationScope() {
  const originals = {
    setTimeout: globalThis.setTimeout,
    clearTimeout: globalThis.clearTimeout,
    setInterval: globalThis.setInterval,
    clearInterval: globalThis.clearInterval,
    setImmediate: globalThis.setImmediate,
    clearImmediate: globalThis.clearImmediate,
    stdoutWrite: process.stdout.write,
  }
  const timers = new Set()
  const intervals = new Set()
  const immediates = new Set()
  const listeners = new Map(process.eventNames().map((event) => [event, new Set(process.listeners(event))]))
  process.stdout.write = (chunk, encoding, callback) => {
    const value = typeof chunk === 'string' ? chunk : Buffer.from(chunk).toString(typeof encoding === 'string' ? encoding : undefined)
    process.stderr.write(`[plugin stdout] ${value}`)
    if (typeof encoding === 'function') encoding()
    if (typeof callback === 'function') callback()
    return true
  }
  globalThis.setTimeout = (callback, delay, ...args) => {
    let handle
    handle = originals.setTimeout((...values) => { timers.delete(handle); callback(...values) }, delay, ...args)
    timers.add(handle)
    return handle
  }
  globalThis.clearTimeout = (handle) => { timers.delete(handle); return originals.clearTimeout(handle) }
  globalThis.setInterval = (callback, delay, ...args) => {
    const handle = originals.setInterval(callback, delay, ...args)
    intervals.add(handle)
    return handle
  }
  globalThis.clearInterval = (handle) => { intervals.delete(handle); return originals.clearInterval(handle) }
  globalThis.setImmediate = (callback, ...args) => {
    let handle
    handle = originals.setImmediate((...values) => { immediates.delete(handle); callback(...values) }, ...args)
    immediates.add(handle)
    return handle
  }
  globalThis.clearImmediate = (handle) => { immediates.delete(handle); return originals.clearImmediate(handle) }
  return { dispose() {
    globalThis.setTimeout = originals.setTimeout
    globalThis.clearTimeout = originals.clearTimeout
    globalThis.setInterval = originals.setInterval
    globalThis.clearInterval = originals.clearInterval
    globalThis.setImmediate = originals.setImmediate
    globalThis.clearImmediate = originals.clearImmediate
    process.stdout.write = originals.stdoutWrite
    for (const handle of timers) originals.clearTimeout(handle)
    for (const handle of intervals) originals.clearInterval(handle)
    for (const handle of immediates) originals.clearImmediate(handle)
    for (const event of process.eventNames()) {
      const before = listeners.get(event) ?? new Set()
      for (const listener of process.listeners(event)) if (!before.has(listener)) process.removeListener(event, listener)
    }
  } }
}

function rpcError(code, message) { return { code, message } }
class HostCallError extends Error { constructor(code, message) { super(message); this.code = code } }
function write(message) {
  const encoded = JSON.stringify(message)
  if (Buffer.byteLength(encoded) > MAX_MESSAGE_BYTES) throw new Error('Message exceeds 8 MiB')
  protocolWrite(`${encoded}\n`)
}
function notify(method, params) { write({ jsonrpc: '2.0', method, params }) }
function reply(id, result, error) { write(error ? { jsonrpc: '2.0', id, error } : { jsonrpc: '2.0', id, result }) }
