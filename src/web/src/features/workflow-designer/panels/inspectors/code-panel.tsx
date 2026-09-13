import JSON5 from "json5";
import { Plus, Trash2 } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "../../../../shared/ui/button";
import { Input } from "../../../../shared/ui/input";
import { Textarea } from "../../../../shared/ui/textarea";
import { CodeEditor } from "../../forms/code-editor";
import type { JsonSchemaProperty, NodeManifest } from "../../model/types";
import {
  CommonPanelSections,
  ModeCardsGroup,
  OutputContractHint,
  PanelHint,
  PanelSection,
  ParameterControl,
  ResourceSelectorFields,
  type ActionPanelProps,
} from "./panel-shell";

type PortRange = { from: number; to: number };
type Destination = { target: string; ports: PortRange[] };
type NetworkPolicy = { mode: "deny" | "allowlist"; destinations: Destination[] };

export function CodePanel(panel: ActionPanelProps) {
  const { t } = useTranslation();
  const example = isRecord(panel.data.parameters.outputExample) ? panel.data.parameters.outputExample : {};
  const effectiveManifest: NodeManifest = {
    ...panel.manifest,
    outputSchema: {
      ...(panel.manifest.outputSchema as Record<string, unknown>),
      properties: {
        ...((panel.manifest.outputSchema as { properties?: Record<string, unknown> }).properties ?? {}),
        structuredOutput: inferExampleSchema(example),
      },
    },
  };
  return (
    <div className="space-y-5" data-testid="code-panel">
      <PanelSection description={t("studio.panels.code.environmentDescription")} title={t("studio.panels.code.environment")} testId="code-environment-section">
        <div className="space-y-4">
          <ParameterControl name="runner" panel={panel} />
          <NetworkPolicyEditor panel={panel} />
          <ResourceSelectorFields panel={panel} />
        </div>
      </PanelSection>
      <PanelSection title={t("studio.panels.code.inputs")} testId="code-inputs-section">
        <ParameterControl name="inputs" panel={panel} />
      </PanelSection>
      <PanelSection title={t("studio.panels.code.source")} testId="code-source-section">
        <ParameterControl name="source" panel={panel} />
      </PanelSection>
      <PanelSection title={t("studio.panels.code.output")} testId="code-output-section">
        <OutputExampleEditor key={panel.sourceNodeId} panel={panel} />
        <OutputContractHint manifest={effectiveManifest} />
      </PanelSection>
      <CommonPanelSections panel={panel} />
    </div>
  );
}

function NetworkPolicyEditor({ panel }: { panel: ActionPanelProps }) {
  const { t } = useTranslation();
  const policy = normalizePolicy(panel.data.parameters.networkPolicy);
  const update = (networkPolicy: NetworkPolicy) => panel.onChange({ parameters: { networkPolicy } });
  const setMode = (mode: string) => update(mode === "allowlist"
    ? { mode: "allowlist", destinations: policy.destinations.length ? policy.destinations : [{ target: "", ports: [{ from: 443, to: 443 }] }] }
    : { mode: "deny", destinations: [] });
  return <div className="space-y-3" data-field-path="parameters.networkPolicy" data-testid="code-network-policy">
    <div className="text-xs text-muted-foreground">{t("studio.panels.code.networkPolicy")}</div>
    <ModeCardsGroup onChange={setMode} options={[
      { value: "deny", label: t("studio.panels.code.networkDeny"), description: t("studio.panels.code.networkDenyHint") },
      { value: "allowlist", label: t("studio.panels.code.networkAllowlist"), description: t("studio.panels.code.networkAllowlistHint") },
    ]} value={policy.mode} />
    {policy.mode === "allowlist" && <div className="space-y-2">
      {policy.destinations.map((destination, index) => <DestinationRow destination={destination} key={index} onChange={(next) => update({ ...policy, destinations: policy.destinations.map((item, current) => current === index ? next : item) })} onRemove={() => update({ ...policy, destinations: policy.destinations.filter((_, current) => current !== index) })} />)}
      <Button onClick={() => update({ ...policy, destinations: [...policy.destinations, { target: "", ports: [{ from: 443, to: 443 }] }] })} size="sm" variant="ghost"><Plus className="size-3.5" />{t("studio.panels.code.addDestination")}</Button>
      <ProxyExamples runner={String(panel.data.parameters.runner ?? "python")} />
    </div>}
  </div>;
}

function DestinationRow({ destination, onChange, onRemove }: { destination: Destination; onChange: (value: Destination) => void; onRemove: () => void }) {
  const { t } = useTranslation();
  const [ports, setPorts] = useState(formatPorts(destination.ports));
  useEffect(() => setPorts(formatPorts(destination.ports)), [destination.ports]);
  return <div className="grid grid-cols-[minmax(0,1.4fr)_minmax(110px,0.8fr)_32px] gap-2 rounded-md border border-border/70 p-2">
    <Input aria-label={t("studio.panels.code.destination")} onChange={(event) => onChange({ ...destination, target: event.target.value })} placeholder="api.example.com / *.example.com / 10.0.0.0/8" value={destination.target} />
    <Input aria-label={t("studio.panels.code.ports")} onBlur={() => onChange({ ...destination, ports: parsePorts(ports) })} onChange={(event) => setPorts(event.target.value)} placeholder="443, 8000-8010" value={ports} />
    <Button aria-label={t("common.delete")} onClick={onRemove} size="icon" variant="ghost"><Trash2 className="size-3.5" /></Button>
  </div>;
}

function ProxyExamples({ runner }: { runner: string }) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const example = runner === "javascript"
    ? `const https = require('https')
const proxy = new URL(process.env.AGENTX_TCP_PROXY_URL)
const auth = Buffer.from(\`${'${proxy.username}:${proxy.password}'}\`).toString('base64')
const req = https.request({ hostname: proxy.hostname, port: proxy.port, method: 'CONNECT', path: 'db.example.com:5432', headers: { 'Proxy-Authorization': \`Basic ${'${auth}'}\` } })
req.on('connect', (_res, socket) => { /* use socket as the TCP stream */ })
req.end()`
    : runner === "shell"
      ? `curl --proxy "$AGENTX_TCP_PROXY_URL" --proxytunnel telnet://db.example.com:5432`
      : `import base64, os, socket, ssl
from urllib.parse import urlparse
proxy = urlparse(os.environ["AGENTX_TCP_PROXY_URL"])
auth = base64.b64encode(f"{proxy.username}:{proxy.password}".encode()).decode()
raw = socket.create_connection((proxy.hostname, proxy.port))
sock = ssl.create_default_context(cafile=os.environ["SSL_CERT_FILE"]).wrap_socket(raw, server_hostname=proxy.hostname)
sock.sendall(f"CONNECT db.example.com:5432 HTTP/1.1\\r\\nHost: db.example.com:5432\\r\\nProxy-Authorization: Basic {auth}\\r\\n\\r\\n".encode())
response = b""
while b"\\r\\n\\r\\n" not in response:
    response += sock.recv(4096)
if not response.startswith(b"HTTP/1.1 200"):
    raise RuntimeError(response.split(b"\\r\\n", 1)[0].decode())
# sock is now the raw TCP stream to db.example.com:5432`;
  return <details className="overflow-hidden rounded-md border border-border bg-muted/20" onToggle={(event) => setOpen(event.currentTarget.open)}><summary className="cursor-pointer px-3 py-2 text-[11px] font-medium">{t("studio.panels.code.proxyExample")}</summary>{open && <div className="border-t border-border bg-canvas"><div className="flex justify-end border-b border-border px-2 py-1"><Button onClick={() => void navigator.clipboard.writeText(example)} size="sm" variant="ghost">{t("studio.panels.code.copyExample")}</Button></div><CodeEditor height="260px" language={runner === "javascript" ? "javascript" : runner === "shell" ? "shell" : "python"} onChange={() => undefined} readOnly value={example} /></div>}</details>;
}

function OutputExampleEditor({ panel }: { panel: ActionPanelProps }) {
  const { t } = useTranslation();
  const current = isRecord(panel.data.parameters.outputExample) ? panel.data.parameters.outputExample : {};
  const [source, setSource] = useState(() => JSON.stringify(current, null, 2));
  const [error, setError] = useState<string>();
  const commit = (next: string) => {
    try {
      const parsed = JSON5.parse(next) as unknown;
      if (!isRecord(parsed)) throw new Error(t("studio.panels.code.outputObjectRequired"));
      setError(undefined);
      panel.onChange({ parameters: { outputExample: parsed } });
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    }
  };
  return <div data-field-path="parameters.outputExample" data-testid="code-output-example">
    <label className="mb-1.5 block text-xs text-muted-foreground">{t("studio.panels.code.outputExample")}</label>
    <p className="mb-2 text-[10px] leading-4 text-muted-foreground">{t("studio.panels.code.outputExampleDescription")}</p>
    <Textarea
      aria-label={t("studio.panels.code.outputExample")}
      className="min-h-[180px] resize-y bg-canvas font-mono text-xs shadow-inner"
      onBlur={(event) => commit(event.target.value)}
      onChange={(event) => setSource(event.target.value)}
      spellCheck={false}
      value={source}
    />
    {error ? <p className="mt-1 text-[10px] text-danger" role="alert">{error}</p> : <PanelHint>{t("studio.panels.code.outputExampleHint")}</PanelHint>}
  </div>;
}

function normalizePolicy(value: unknown): NetworkPolicy {
  if (!isRecord(value) || value.mode !== "allowlist" || !Array.isArray(value.destinations)) return { mode: "deny", destinations: [] };
  return {
    mode: "allowlist",
    destinations: value.destinations.filter(isRecord).map((entry) => ({
      target: typeof entry.target === "string" ? entry.target : "",
      ports: Array.isArray(entry.ports) ? entry.ports.filter(isRecord).map((port) => ({ from: Number(port.from), to: Number(port.to) })) : [],
    })),
  };
}

function parsePorts(value: string): PortRange[] {
  return value.split(",").map((item) => item.trim()).filter(Boolean).map((item) => {
    const [from, to = from] = item.split("-", 2).map(Number);
    return { from, to };
  }).filter((range) => Number.isInteger(range.from) && Number.isInteger(range.to) && range.from >= 1 && range.to <= 65535 && range.from <= range.to).slice(0, 8);
}

function formatPorts(ports: PortRange[]) {
  return ports.map((range) => range.from === range.to ? String(range.from) : `${range.from}-${range.to}`).join(", ");
}

function inferExampleSchema(value: unknown): JsonSchemaProperty {
  if (value === null) return {};
  if (Array.isArray(value)) {
    const schemas = value.map(inferExampleSchema);
    const first = schemas[0];
    const homogeneous = first && schemas.every((schema) => JSON.stringify(schema) === JSON.stringify(first));
    return { type: "array", items: homogeneous ? first : {} };
  }
  if (isRecord(value)) return { type: "object", properties: Object.fromEntries(Object.entries(value).map(([name, child]) => [name, inferExampleSchema(child)])), required: Object.keys(value), additionalProperties: false };
  if (typeof value === "boolean") return { type: "boolean" };
  if (typeof value === "number") return { type: Number.isInteger(value) ? "integer" : "number" };
  return { type: "string" };
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value && typeof value === "object" && !Array.isArray(value));
}
