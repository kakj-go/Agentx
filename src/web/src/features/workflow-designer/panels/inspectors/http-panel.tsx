import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Plus, Trash2 } from "lucide-react";

import { Button } from "../../../../shared/ui/button";
import { Input } from "../../../../shared/ui/input";
import { Select } from "../../../../shared/ui/select";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "../../../../shared/ui/tabs";
import { asInputBinding, TemplateInput } from "../../forms/binding-inputs";
import {
  BindingSlotResourceFields,
  CommonPanelSections,
  OutputContractHint,
  PanelSection,
  ParameterControl,
  type ActionPanelProps,
  resourceOptionsFor,
} from "./panel-shell";

export function HttpPanel(panel: ActionPanelProps) {
  const { t } = useTranslation();
  const [requestTab, setRequestTab] = useState("query");
  const credential = panel.data.resourceReferences.find((reference) => reference.bindingRole === "credential");
  const credentialType = credential
    ? resourceOptionsFor(panel.resources, "credential", "use").find((option) => option.value === credential.resourceId)?.detail
    : undefined;
  useEffect(() => {
    if (credentialType && credentialType !== "api_key" && panel.data.parameters.apiKeyPlacement) {
      panel.onChange({ parameters: { apiKeyPlacement: undefined } });
    }
  }, [credentialType, panel]);
  return (
    <div className="space-y-5" data-testid="http-panel">
      <PanelSection title={t("studio.panels.http.request")} testId="http-request-section">
        <div className="grid grid-cols-[110px_minmax(0,1fr)] items-start gap-2">
          <ParameterControl name="method" panel={panel} />
          <ParameterControl name="url" panel={panel} />
        </div>
        <Tabs
          className="mt-4 flex min-h-0 flex-col"
          data-testid="http-request-tabs"
          onValueChange={setRequestTab}
          value={requestTab}
        >
          <TabsList className="h-9 w-full justify-start gap-4 px-1">
            <TabsTrigger className="text-[11px]" value="query">
              {t("studio.panels.http.query")}
            </TabsTrigger>
            <TabsTrigger className="text-[11px]" value="headers">
              {t("studio.panels.http.headers")}
            </TabsTrigger>
            <TabsTrigger className="text-[11px]" value="body">
              {t("studio.panels.http.body")}
            </TabsTrigger>
          </TabsList>
          <TabsContent className="pt-3" value="query">
            <KeyValueRows name="query" panel={panel} />
          </TabsContent>
          <TabsContent className="pt-3" value="headers">
            <KeyValueRows name="headers" panel={panel} />
          </TabsContent>
          <TabsContent className="pt-3" value="body">
            <ParameterControl name="body" panel={panel} />
          </TabsContent>
        </Tabs>
      </PanelSection>
      <PanelSection title={t("studio.panels.http.authentication")} testId="http-auth-section">
        <BindingSlotResourceFields panel={panel} />
        {credentialType === "api_key" && <ApiKeyPlacement panel={panel} />}
      </PanelSection>
      <PanelSection title={t("studio.panels.http.timeout")} testId="http-timeout-section">
        <label className="block text-xs" data-field-path="settings.timeoutMs">
          <span className="mb-1.5 block text-muted-foreground">{t("studio.panels.http.timeoutSeconds")}</span>
          <Input
            data-testid="http-timeout-seconds"
            min={1}
            onChange={(event) => {
              const seconds = Number(event.target.value);
              panel.onChange({ settings: { ...panel.data.settings, timeoutMs: Math.max(1, seconds) * 1_000 } });
            }}
            type="number"
            value={Math.max(1, Math.round(Number(panel.data.settings.timeoutMs ?? panel.manifest.defaultTimeoutMs ?? 30_000) / 1_000))}
          />
        </label>
      </PanelSection>
      <PanelSection title={t("studio.panels.http.output")} testId="http-output-section">
        <OutputContractHint manifest={panel.manifest} />
      </PanelSection>
      <CommonPanelSections panel={panel} />
    </div>
  );
}

function KeyValueRows({ name, panel }: { name: "query" | "headers"; panel: ActionPanelProps }) {
  const { t } = useTranslation();
  const rows = Array.isArray(panel.data.parameters[name]) ? panel.data.parameters[name] as Array<{ name?: string; value?: unknown }> : [];
  const update = (next: typeof rows) => panel.onChange({ parameters: { [name]: next } });
  return <div className="space-y-2" data-testid={`http-${name}-rows`}>
    {rows.map((row, index) => <div className="grid grid-cols-[minmax(100px,.8fr)_minmax(0,1.2fr)_32px] gap-2" key={index}>
      <Input aria-label={t("studio.inspector.name")} onChange={(event) => update(rows.map((item, current) => current === index ? { ...item, name: event.target.value } : item))} value={row.name ?? ""} />
      <TemplateInput allowedNamespaces={["inputs", "outputs", "contexts", "execution", "item", "loop"]} catalog={panel.referenceCatalog} onChange={(value) => update(rows.map((item, current) => current === index ? { ...item, value } : item))} value={asInputBinding(row.value)} />
      <Button aria-label={t("common.delete")} onClick={() => update(rows.filter((_, current) => current !== index))} size="icon" variant="ghost"><Trash2 className="size-3.5" /></Button>
    </div>)}
    <Button onClick={() => update([...rows, { name: "", value: { kind: "template", segments: [] } }])} size="sm" variant="ghost"><Plus className="size-3.5" />{t("common.create")}</Button>
  </div>;
}

function ApiKeyPlacement({ panel }: { panel: ActionPanelProps }) {
  const value = panel.data.parameters.apiKeyPlacement as { in?: string; name?: string } | undefined;
  const update = (patch: { in?: string; name?: string }) => panel.onChange({ parameters: { apiKeyPlacement: { in: value?.in ?? "header", name: value?.name ?? "x-api-key", ...patch } } });
  return <div className="mt-3 grid grid-cols-[110px_minmax(0,1fr)] gap-2" data-testid="http-api-key-placement">
    <Select onValueChange={(location) => update({ in: location })} options={[{ value: "header", label: "Header" }, { value: "query", label: "Query" }]} value={value?.in ?? "header"} />
    <Input onChange={(event) => update({ name: event.target.value })} value={value?.name ?? "x-api-key"} />
  </div>;
}
