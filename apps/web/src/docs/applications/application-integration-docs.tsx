import { AlertTriangle, BookOpen, KeyRound, ShieldCheck, Webhook, X } from 'lucide-react'
import { useState, type ReactNode } from 'react'

import { JsonSchemaViewer } from '../../shared/components/json-schema-viewer'
import { cn } from '../../shared/lib/cn'
import { Badge } from '../../shared/ui/badge'
import { Button } from '../../shared/ui/button'
import { Dialog, DialogClose, DialogContent } from '../../shared/ui/dialog'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '../../shared/ui/tabs'
import { apiKeyExample, applicationInvocationUrl, integrationLanguages, type ApiKeyEndpointId, type IntegrationLanguage, webhookExample, webhookUrl } from './examples'
import { CodeSnippet } from './code-snippet'
import { useApplicationIntegrationDocsText } from './use-application-integration-docs'

export type ApplicationIntegrationDoc = { kind: 'apiKey' } | { kind: 'webhook'; name?: string; path?: string }

type Props = {
  activeDeployment?: { inputSchema: unknown; outputSchema: unknown }
  applicationSlug: string
  document: ApplicationIntegrationDoc
  onOpenChange: (open: boolean) => void
  open: boolean
  runtimeBaseUrl: string
}

type RequestField = readonly [location: string, name: string, type: string, required: boolean, description: string]
type ResponseField = readonly [name: string, type: string, required: boolean, description: string]
type EndpointDoc = {
  id: ApiKeyEndpointId
  method: string
  path: string
  title: string
  description: string
  responseKind: 'invocation' | 'session' | 'accepted' | 'events'
  fields: readonly RequestField[]
}

export function ApplicationIntegrationDocs({ activeDeployment, applicationSlug, document, onOpenChange, open, runtimeBaseUrl }: Props) {
  const text = useApplicationIntegrationDocsText()
  const apiKey = document.kind === 'apiKey'
  const doc = apiKey ? text.apiKey : text.webhook
  const Icon = apiKey ? KeyRound : Webhook
  const endpoint = apiKey
    ? applicationInvocationUrl(runtimeBaseUrl, applicationSlug)
    : document.path ? webhookUrl(runtimeBaseUrl, document.path) : `${runtimeBaseUrl.replace(/\/$/, '')}/gateway/v1/webhooks/{public_id}`

  return <Dialog onOpenChange={onOpenChange} open={open}>
    <DialogContent className="w-[min(1180px,calc(100vw-48px))]" description={doc.description} title={doc.title}>
      <header className="flex items-start gap-3 border-b border-border px-6 py-5">
        <span className="grid size-10 shrink-0 place-items-center rounded-lg bg-primary/10 text-primary"><Icon className="size-5" /></span>
        <div className="min-w-0"><h2 className="text-base font-semibold">{doc.title}</h2><p className="mt-1 text-xs leading-5 text-muted-foreground">{doc.description}</p></div>
        <DialogClose asChild><Button aria-label={text.close} className="ml-auto" size="icon" variant="ghost"><X className="size-4" /></Button></DialogClose>
      </header>
      <Tabs defaultValue="overview">
        <TabsList className="border-b border-border px-6">
          <TabsTrigger value="overview">{text.overview}</TabsTrigger>
          <TabsTrigger value="reference">{text.apiReference}</TabsTrigger>
          <TabsTrigger value="schemas">{text.schemas}</TabsTrigger>
          <TabsTrigger value="security">{text.security}</TabsTrigger>
          <TabsTrigger value="errors">{text.errors}</TabsTrigger>
        </TabsList>
        <TabsContent className="space-y-6 p-6" value="overview">
          <p className="text-xs leading-6 text-muted-foreground">{doc.intro}</p>
          {!activeDeployment && <Notice>{text.noActiveDeployment}</Notice>}
          {!apiKey && !document.path && <Notice>{text.webhook.noSpecificEndpoint}</Notice>}
          <Endpoint method="POST" value={endpoint} />
          <Steps title={doc.stepsTitle} values={doc.steps} />
          {apiKey
            ? <LanguageExamples endpointId="createInvocation" inputSchema={activeDeployment?.inputSchema} runtimeBaseUrl={runtimeBaseUrl} slug={applicationSlug} />
            : <WebhookExamples inputSchema={activeDeployment?.inputSchema} path={document.path} runtimeBaseUrl={runtimeBaseUrl} />}
        </TabsContent>
        <TabsContent className="p-6" value="reference">
          {apiKey
            ? <ApiKeyReference inputSchema={activeDeployment?.inputSchema} runtimeBaseUrl={runtimeBaseUrl} slug={applicationSlug} />
            : <WebhookReference inputSchema={activeDeployment?.inputSchema} path={document.path} runtimeBaseUrl={runtimeBaseUrl} />}
        </TabsContent>
        <TabsContent className="space-y-6 p-6" value="schemas">
          {!activeDeployment && <Notice>{text.noActiveDeployment}</Notice>}
          {activeDeployment && <p className="rounded-md border border-primary/20 bg-primary/5 px-3 py-2.5 text-[11px] leading-5 text-muted-foreground">{text.schemaSource}</p>}
          <DocumentSection title={text.requestSchema}><JsonSchemaViewer schema={activeDeployment?.inputSchema} /></DocumentSection>
          <DocumentSection title={text.responseSchema}><JsonSchemaViewer schema={activeDeployment?.outputSchema} /></DocumentSection>
        </TabsContent>
        <TabsContent className="space-y-5 p-6" value="security">
          {!apiKey && <DocumentSection title={text.webhook.signatureTitle}><code className="block rounded-md border border-border bg-canvas p-4 text-xs text-primary">{text.webhook.signatureFormula}</code></DocumentSection>}
          <div className="grid gap-3 md:grid-cols-2">{doc.securityItems.map(([title, description]) => <article className="rounded-lg border border-border p-4" key={title}><ShieldCheck className="size-4 text-primary" /><h3 className="mt-3 text-xs font-semibold">{title}</h3><p className="mt-1 text-[11px] leading-5 text-muted-foreground">{description}</p></article>)}</div>
        </TabsContent>
        <TabsContent className="p-6" value="errors"><ErrorTable rows={text.commonErrors} /></TabsContent>
      </Tabs>
    </DialogContent>
  </Dialog>
}

function ApiKeyReference({ inputSchema, runtimeBaseUrl, slug }: { inputSchema: unknown; runtimeBaseUrl: string; slug: string }) {
  const text = useApplicationIntegrationDocsText()
  const endpoints = text.apiKey.endpoints as readonly EndpointDoc[]
  const [selectedId, setSelectedId] = useState<ApiKeyEndpointId>('createInvocation')
  const endpoint = endpoints.find((item) => item.id === selectedId) ?? endpoints[0]
  const responseFields = text.apiKey.responses[endpoint.responseKind] as readonly ResponseField[]

  return <div className="grid gap-6 lg:grid-cols-[250px_minmax(0,1fr)]">
    <aside className="h-fit overflow-hidden rounded-lg border border-border">
      {endpoints.map((item) => <button className={cn('flex w-full items-start gap-2 border-b border-border px-3 py-3 text-left last:border-0 hover:bg-muted/40', item.id === endpoint.id && 'bg-primary/5')} key={item.id} onClick={() => setSelectedId(item.id)} type="button">
        <Badge className="mt-0.5 w-11 justify-center px-1 font-mono text-[9px]" tone={item.method === 'GET' ? 'neutral' : 'primary'}>{item.method}</Badge>
        <span><strong className="block text-xs font-medium">{item.title}</strong><code className="mt-1 block break-all text-[9px] text-muted-foreground">{item.path}</code></span>
      </button>)}
    </aside>
    <div className="min-w-0 space-y-6">
      <section><div className="flex items-center gap-2"><Badge tone={endpoint.method === 'GET' ? 'neutral' : 'primary'}>{endpoint.method}</Badge><h3 className="text-sm font-semibold">{endpoint.title}</h3></div><p className="mt-2 text-xs leading-5 text-muted-foreground">{endpoint.description}</p><code className="mt-3 block break-all rounded-md border border-border bg-canvas px-3 py-2.5 text-[11px]">{endpoint.path}</code></section>
      <RequestFieldTable rows={endpoint.fields} title={text.fields} />
      <ResponseFieldTable rows={responseFields} title={text.responseFields} />
      <DocumentSection title={text.requestExample}><LanguageExamples endpointId={endpoint.id} inputSchema={inputSchema} runtimeBaseUrl={runtimeBaseUrl} slug={slug} /></DocumentSection>
    </div>
  </div>
}

function WebhookReference({ inputSchema, path, runtimeBaseUrl }: { inputSchema: unknown; path?: string; runtimeBaseUrl: string }) {
  const text = useApplicationIntegrationDocsText()
  return <div className="space-y-6">
    {!path && <Notice>{text.webhook.noSpecificEndpoint}</Notice>}
    <Endpoint method="POST" value={path ? webhookUrl(runtimeBaseUrl, path) : `${runtimeBaseUrl.replace(/\/$/, '')}/gateway/v1/webhooks/{public_id}`} />
    <RequestFieldTable rows={text.webhook.fields} title={text.fields} />
    <ResponseFieldTable rows={text.apiKey.responses.invocation} title={text.responseFields} />
    <DocumentSection title={text.webhook.signingTitle}><WebhookExamples inputSchema={inputSchema} path={path} runtimeBaseUrl={runtimeBaseUrl} /></DocumentSection>
  </div>
}

function LanguageExamples({ endpointId, runtimeBaseUrl, slug, inputSchema }: { endpointId: ApiKeyEndpointId; runtimeBaseUrl: string; slug: string; inputSchema: unknown }) {
  return <CodeLanguageTabs getCode={(language) => apiKeyExample(endpointId, language, runtimeBaseUrl, slug, inputSchema)} />
}

function WebhookExamples({ runtimeBaseUrl, path, inputSchema }: { runtimeBaseUrl: string; path?: string; inputSchema: unknown }) {
  return <CodeLanguageTabs getCode={(language) => webhookExample(language, runtimeBaseUrl, path, inputSchema)} />
}

function CodeLanguageTabs({ getCode }: { getCode: (language: IntegrationLanguage) => string }) {
  const text = useApplicationIntegrationDocsText()
  return <Tabs defaultValue="curl">
    <TabsList className="h-10 rounded-md bg-muted/40 px-3">{integrationLanguages.map((language) => <TabsTrigger className="text-xs" key={language} value={language}>{text.languages[language]}</TabsTrigger>)}</TabsList>
    {integrationLanguages.map((language) => <TabsContent className="pt-3" key={language} value={language}><CodeSnippet code={getCode(language)} language={text.languages[language]} /></TabsContent>)}
  </Tabs>
}

function Steps({ title, values }: { title: string; values: readonly string[] }) {
  return <DocumentSection title={title}><ol className="grid gap-2">{values.map((step, index) => <li className="flex gap-3 text-xs leading-5 text-muted-foreground" key={step}><Badge className="h-5 min-w-5 justify-center px-1.5" tone="primary">{index + 1}</Badge><span>{step}</span></li>)}</ol></DocumentSection>
}

function Endpoint({ method, value }: { method: string; value: string }) {
  const text = useApplicationIntegrationDocsText()
  return <DocumentSection title={text.endpoint}><div className="flex items-center gap-2 rounded-md border border-border bg-canvas px-3 py-2.5"><Badge tone="primary">{method}</Badge><code className="min-w-0 break-all text-[11px]">{value}</code></div></DocumentSection>
}

function RequestFieldTable({ rows, title }: { rows: readonly RequestField[]; title: string }) {
  const text = useApplicationIntegrationDocsText()
  return <DocumentSection title={title}><div className="overflow-x-auto rounded-lg border border-border"><div className="min-w-[760px]"><div className="grid grid-cols-[80px_150px_130px_70px_minmax(260px,1fr)] gap-3 bg-muted/45 px-3 py-2 text-[10px] font-medium uppercase text-muted-foreground"><span>{text.table.location}</span><span>{text.table.name}</span><span>{text.table.type}</span><span>{text.table.required}</span><span>{text.table.description}</span></div><div className="divide-y divide-border">{rows.map((row) => <div className="grid grid-cols-[80px_150px_130px_70px_minmax(260px,1fr)] gap-3 px-3 py-2.5 text-[11px]" key={`${row[0]}-${row[1]}`}><span>{row[0]}</span><code className="break-all text-primary">{row[1]}</code><code className="break-all text-muted-foreground">{row[2]}</code><span>{row[3] ? text.table.yes : text.table.no}</span><span className="leading-5 text-muted-foreground">{row[4]}</span></div>)}</div></div></div></DocumentSection>
}

function ResponseFieldTable({ rows, title }: { rows: readonly ResponseField[]; title: string }) {
  const text = useApplicationIntegrationDocsText()
  return <DocumentSection title={title}><div className="overflow-x-auto rounded-lg border border-border"><div className="min-w-[680px]"><div className="grid grid-cols-[170px_140px_70px_minmax(260px,1fr)] gap-3 bg-muted/45 px-3 py-2 text-[10px] font-medium uppercase text-muted-foreground"><span>{text.table.name}</span><span>{text.table.type}</span><span>{text.table.required}</span><span>{text.table.description}</span></div><div className="divide-y divide-border">{rows.map((row) => <div className="grid grid-cols-[170px_140px_70px_minmax(260px,1fr)] gap-3 px-3 py-2.5 text-[11px]" key={row[0]}><code className="break-all text-primary">{row[0]}</code><code className="break-all text-muted-foreground">{row[1]}</code><span>{row[2] ? text.table.yes : text.table.no}</span><span className="leading-5 text-muted-foreground">{row[3]}</span></div>)}</div></div></div></DocumentSection>
}

function ErrorTable({ rows }: { rows: ReadonlyArray<ReadonlyArray<string>> }) {
  const text = useApplicationIntegrationDocsText()
  return <DocumentSection title={text.errors}><div className="overflow-hidden rounded-lg border border-border"><div className="divide-y divide-border">{rows.map(([status, description]) => <div className="grid grid-cols-[70px_minmax(0,1fr)] gap-3 px-3 py-2.5 text-[11px]" key={status}><strong className="font-mono text-foreground">{status}</strong><span className="leading-5 text-muted-foreground">{description}</span></div>)}</div></div></DocumentSection>
}

function DocumentSection({ title, children }: { title: string; children: ReactNode }) {
  return <section><div className="mb-3 flex items-center gap-2"><BookOpen className="size-3.5 text-primary" /><h3 className="text-xs font-semibold">{title}</h3></div>{children}</section>
}

function Notice({ children }: { children: ReactNode }) {
  return <div className="flex gap-2 rounded-lg border border-warning/30 bg-warning/10 p-3 text-[11px] leading-5 text-muted-foreground"><AlertTriangle className="mt-0.5 size-4 shrink-0 text-warning" />{children}</div>
}
