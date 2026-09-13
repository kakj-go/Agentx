export const PLUGIN_PROTOCOL_VERSION = 1 as const
export const PLUGIN_SDK_API_VERSION = 1 as const

export type Json = null | boolean | number | string | Json[] | { [key: string]: Json }
export type ItemSource = { nodeExecutionId: string; nodeId: string; runIndex: number; outputIndex: number; itemIndex: number }
export type Item = { json: Json; lineage?: ItemSource[]; metadata?: Record<string, Json>; binary?: Record<string, ArtifactRef> }
export type ArtifactRef = { artifactId: string; fileName: string; contentType: string; sizeBytes: number; sha256: string }
export type NodeOutputs = Record<string, Item[]>
export type HttpRequest = { method?: string; url: string; query?: Array<{ name: string; value: Json }>; headers?: Record<string, string>; body?: Json; credentialIndex?: number; idempotencyKey?: string }
export type HostHttpResponse = { status: number; headers?: Record<string, string>; body?: Json; files?: ArtifactRef[]; runtimeCallId?: string }
export type ModelRequest = { prompt?: string; userQuestion?: string; responseMode?: 'text' | 'structured'; structuredSchema?: Json; resourceIndex?: number }
export type CredentialDescriptor = { index: number; resourceId: string; resourceVersion: string; credentialType: string; allowedOperations: string[] }
export type ArtifactInput = { bytesBase64: string; fileName: string; contentType: string }

export type TraceContent = { type: string; version: number; data: Json; label?: string }
export type TraceSpan = {
  setAttribute(name: string, value: Json): void
  content(value: TraceContent): void
  event(name: string, attributes?: Record<string, Json>): void
}

export type ExecuteContext<P extends Json = Json> = {
  inputs: Record<string, Item[]>
  parameters: P
  perItemParameters: P[]
  stringConversions: Json[]
  context: Json
  execution: { nodeType: string; nodeVersion: number; packageId: string; packageVersion: string; bundleDigest: string; executionId: string; nodeExecutionId: string; attemptId: string; runIndex: number; iterationIndex: number; idempotencyKey: string; deadline: string }
  signal: AbortSignal
  trace: { span<T>(name: string, body: (span: TraceSpan) => Promise<T> | T): Promise<T> }
  items: { fromJson(values: Json[]): Item[] }
  http(request: HttpRequest): Promise<HostHttpResponse>
  model(request: ModelRequest): Promise<Json>
  credentials: { list(): Promise<CredentialDescriptor[]> }
  artifacts: { put(input: ArtifactInput): Promise<ArtifactRef> }
}

export type Completed = { status: 'completed'; outputs: NodeOutputs }
export type Failed = { status: 'failed'; code: string; message: string; retryable?: boolean; details?: Json }
export type ExecuteResult = Completed | Failed
export type Execute<P extends Json = Json> = (context: ExecuteContext<P>) => Promise<ExecuteResult> | ExecuteResult

export function completed(outputs: NodeOutputs): Completed { return { status: 'completed', outputs } }
export function failed(code: string, message: string, retryable = false, details?: Json): Failed { return { status: 'failed', code, message, retryable, details } }
export function defineExecute<P extends Json>(execute: Execute<P>): Execute<P> { return execute }
export type ResolvedDefinition = { status: 'complete' | 'incomplete' | 'invalid'; inputPorts?: Array<{ name: string; kind: 'main' | 'error'; required?: boolean; variadic?: boolean }>; outputPorts?: Array<{ name: string; kind: 'main' | 'error'; required?: boolean; variadic?: boolean }>; outputSchema?: Json; outputPortSchemas?: Record<string, Json>; issues?: Array<{ path: string; code: string; message: string }> }
export type ResolveDefinition<P extends Json = Json> = (configuration: P, upstreamContracts: Record<string, Json>, context: { nodeType: string }) => Promise<ResolvedDefinition> | ResolvedDefinition
export type ProviderOption = { value: string; label: string; description?: string }
export type ProviderPage = { items: ProviderOption[]; nextCursor?: string | null }
export type ProviderContext = {
  signal: AbortSignal
  http(request: HttpRequest): Promise<HostHttpResponse>
  model(request: ModelRequest): Promise<Json>
  credentials: { list(): Promise<CredentialDescriptor[]> }
  artifacts: { put(input: ArtifactInput): Promise<ArtifactRef> }
}
export type Provider = (input: { search?: string; limit?: number; cursor?: string; parameters?: Json }, context: ProviderContext) => Promise<ProviderOption[] | ProviderPage> | ProviderOption[] | ProviderPage
