import { useMutation, useQuery } from "@tanstack/react-query";
import { ChevronDown, ChevronUp, MoreHorizontal, Play, Plus, Power, Settings2, Trash2, X } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { apiRequest } from "../../../shared/api/client";
import { Button } from "../../../shared/ui/button";
import { Dialog, DialogContent } from "../../../shared/ui/dialog";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "../../../shared/ui/dropdown-menu";
import { Input } from "../../../shared/ui/input";
import { Select } from "../../../shared/ui/select";
import {
  Tabs,
  TabsContent,
  TabsList,
  TabsTrigger,
} from "../../../shared/ui/tabs";
import { useProviderOptions } from "../api/use-provider-options";
import { deleteOverlay, saveOverlay } from "../api/studio-api";
import type {
  NodeExecution,
} from "../../../shared/api/types";
import { TraceDetail } from "../../traces/trace-detail";
import { useExecutionTrace } from "../../traces/use-trace";
import type {
  ActionNodeData,
  BindingNodeData,
  NodeManifest,
  ReferenceCatalog,
  ReferenceEntry,
  ResourceOption,
  ResourceRequestContext,
  ResourceType,
  UiField,
} from "../model/types";
import {
  localizeManifest,
  localizedNodeLabel,
} from "../model/manifest-localization";
import {
  ParameterField,
  ReferenceControl,
  SUPPORTED_CONTROLS,
} from "../forms/parameter-field";
import { insertAtSelection } from "../forms/reference-picker/reference-insertion";
import { ResourcePicker } from "../forms/resource-picker";
import { NodeIcon } from "../nodes/node-icon";
import { Badge } from "../../../shared/ui/badge";

type InspectableNodeData = ActionNodeData | BindingNodeData;

export function NodeInspector({
  data,
  nodeId,
  manifest,
  compositeManifests = [],
  resources,
  workflowId,
  executionId,
  referenceCatalog,
  open = true,
  onClose,
  onRun,
  onChange,
  onDelete,
  onOverlayChange,
  fieldErrors = {},
  onValidityChange,
  onResourceAuthorize,
  onResourceRequest,
}: {
  data?: InspectableNodeData;
  nodeId?: string;
  manifest?: NodeManifest;
  compositeManifests?: NodeManifest[];
  resources: Partial<Record<ResourceType, ResourceOption[]>>;
  workflowId?: string;
  executionId?: string;
  referenceCatalog?: ReferenceCatalog;
  open?: boolean;
  onClose?: () => void;
  onRun?: () => void;
  onChange: (data: Partial<ActionNodeData> | Partial<BindingNodeData>) => void;
  onDelete: () => void;
  onOverlayChange?: (nodeId: string, overlayId?: string) => void;
  fieldErrors?: Record<string, string>;
  onValidityChange?: (valid: boolean) => void;
  onResourceAuthorize?: (option: ResourceOption) => Promise<void>;
  onResourceRequest?: (option: ResourceOption, context?: ResourceRequestContext) => Promise<void>;
}) {
  const { t, i18n } = useTranslation();
  const [tab, setTab] = useState("parameters");
  const unsupported = parameterEntries(manifest).some(
    ([name]) =>
      !SUPPORTED_CONTROLS.has(uiFields(manifest)[name]?.control ?? ""),
  );
  const providerOptions = useProviderOptions(manifest);
  const dataKey =
    data?.editorKind === "action"
      ? data.nodeType
      : data?.editorKind === "binding"
        ? data.resourceType
        : undefined;
  const localized = manifest
    ? localizeManifest(manifest, i18n.language)
    : undefined;
  const compositeVersions = compositeManifests.filter(
    (candidate) =>
      candidate.nodeType !== manifest?.nodeType &&
      candidate.parameterSchema["x-agentx-workflowId"] ===
        manifest?.parameterSchema["x-agentx-workflowId"],
  );
  const title =
    data?.editorKind === "action" && manifest
      ? localizedNodeLabel(manifest, data.label, i18n.language)
      : (data?.label ?? "");
  const currentNodeCatalog = useMemo(
    () =>
      data?.editorKind === "action" && manifest && referenceCatalog
        ? addCurrentNodeReferences(referenceCatalog, data, manifest, t("studio.references.currentRawOutput"))
        : referenceCatalog,
    [data, manifest, referenceCatalog, t],
  );
  const translate = (key: string, fallback?: string) =>
    fallback ? t(key, { defaultValue: fallback }) : t(key);
  const nodeRuns = useQuery({
    queryKey: ["studio-node-inspector", executionId, nodeId],
    queryFn: () =>
      apiRequest<{ items: NodeExecution[] }>(
        `/executions/${executionId}/nodes`,
      ),
    enabled: Boolean(executionId && nodeId),
  });
  const trace = useExecutionTrace(executionId, tab === "trace");
  const selectedRuns = useMemo(
    () => nodeRuns.data?.items.filter((node) => node.nodeId === nodeId) ?? [],
    [nodeId, nodeRuns.data?.items],
  );
  const selectedRun = selectedRuns.at(-1);
  const selectedExecutionIds = new Set(selectedRuns.map((run) => run.id));
  const selectedSpan = trace.spans.find(
    (span) => span.spanKind === "node" && Boolean(span.nodeExecutionId && selectedExecutionIds.has(span.nodeExecutionId)),
  );
  const [overlayText, setOverlayText] = useState("{}");
  const overlay = useMutation({
    mutationFn: (kind: string) =>
      saveOverlay(workflowId!, nodeId!, kind, JSON.parse(overlayText)),
    onSuccess: (value) => nodeId && onOverlayChange?.(nodeId, value.id),
  });
  const removeOverlay = useMutation({
    mutationFn: () => deleteOverlay(workflowId!, nodeId!),
    onSuccess: () => nodeId && onOverlayChange?.(nodeId),
  });

  useEffect(
    () => onValidityChange?.(!unsupported),
    [onValidityChange, unsupported],
  );
  useEffect(() => {
    if (selectedRun?.output !== undefined)
      setOverlayText(JSON.stringify(selectedRun.output, null, 2));
  }, [selectedRun?.output]);
  useEffect(() => {
    if (data) setTab("parameters");
  }, [data, dataKey]);

  if (!open || !data) return null;
  return (
    <aside
      className="flex h-full w-[480px] shrink-0 flex-col overflow-hidden border-l border-border bg-surface"
      data-testid="node-details-view"
    >
      <div className="relative z-[101] flex shrink-0 items-center gap-3 border-b border-border bg-surface px-4 py-3">
        <span className="grid size-9 place-items-center rounded-lg bg-primary/10 text-primary">
          <NodeIcon
            className="size-5"
            iconKey={
              manifest?.iconKey ??
              (data.editorKind === "binding" ? data.resourceType : "box")
            }
          />
        </span>
        <div className="min-w-0 flex-1">
          <strong className="block truncate text-sm font-semibold">
            {title}
          </strong>
          <div className="flex min-w-0 items-center gap-2">
            <span className="block truncate text-[10px] text-muted-foreground">
              {data.editorKind === "action"
                ? (localized?.displayName ?? data.nodeType)
                : t(`resourceGrants.resourceTypes.${data.resourceType}`)}
            </span>
            {data.editorKind === "action" && data.disabled && (
              <Badge className="shrink-0 px-1.5 py-0.5 text-[10px]" tone="warning">
                {t("studio.inspector.disabled")}
              </Badge>
            )}
          </div>
        </div>
        {onRun && data.editorKind === "action" && (
          <Button
            aria-label={t("studio.run")}
            onClick={onRun}
            size="icon"
            variant="ghost"
          >
            <Play className="size-4" />
          </Button>
        )}
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button
              aria-label={t("studio.details.more")}
              size="icon"
              variant="ghost"
            >
              <MoreHorizontal className="size-4" />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end">
            {onRun && data.editorKind === "action" && (
              <DropdownMenuItem onSelect={onRun}>
                <Play className="size-4" />
                {t("studio.run")}
              </DropdownMenuItem>
            )}
            {data.editorKind === "action" && (
              <DropdownMenuItem onSelect={() => onChange({ disabled: !data.disabled })}>
                <Power className="size-4" />
                {data.disabled
                  ? t("studio.inspector.enable")
                  : t("studio.inspector.disable")}
              </DropdownMenuItem>
            )}
            <DropdownMenuItem className="text-danger" onSelect={onDelete}>
              <Trash2 className="size-4" />
              {t("studio.inspector.delete")}
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
        <Button
          aria-label={t("studio.close")}
          onClick={onClose}
          size="icon"
          variant="ghost"
        >
          <X className="size-4" />
        </Button>
      </div>
      <Tabs
        className="flex min-h-0 flex-1 flex-col"
        onValueChange={setTab}
        value={tab}
      >
        <TabsList className="h-11 shrink-0 justify-start gap-4 border-b border-border px-4">
          <TabsTrigger className="text-xs" value="parameters">
            {t("studio.details.parameters")}
          </TabsTrigger>
          <TabsTrigger className="text-xs" value="input">
            {t("studio.details.input")}
          </TabsTrigger>
          <TabsTrigger className="text-xs" value="output">
            {t("studio.details.output")}
          </TabsTrigger>
          <TabsTrigger className="text-xs" value="trace">
            {t("studio.details.trace")}
          </TabsTrigger>
        </TabsList>
        <TabsContent
          className="min-h-0 flex-1 overflow-y-auto p-4"
          value="parameters"
        >
          {data.editorKind === "action" &&
            manifest &&
            compositeVersions.length > 0 && (
              <CompositeUpgrade
                current={manifest}
                parameters={data.parameters}
                versions={compositeVersions}
                onChange={onChange}
              />
            )}
          <Parameters
            data={data}
            fieldErrors={fieldErrors}
            localized={localized}
            manifest={manifest}
            onChange={onChange}
            providerOptions={providerOptions}
            referenceCatalog={referenceCatalog}
            currentNodeCatalog={currentNodeCatalog}
            resources={resources}
            onResourceAuthorize={onResourceAuthorize}
            onResourceRequest={onResourceRequest}
            sourceNodeId={nodeId}
            t={translate}
            workflowId={workflowId}
          />
        </TabsContent>
        <TabsContent className="min-h-0 flex-1 overflow-auto p-4" value="input">
          <JsonValue value={selectedRun?.input} />
        </TabsContent>
        <TabsContent
          className="min-h-0 flex-1 overflow-auto p-4"
          value="output"
        >
          <div className="mb-3 flex items-center gap-1">
            <Button
              disabled={overlay.isPending || data.editorKind !== "action"}
              onClick={() => overlay.mutate("pin_data")}
              size="sm"
              variant="secondary"
            >
              {t("studio.runtime.pin")}
            </Button>
            <Button
              disabled={overlay.isPending || data.editorKind !== "action"}
              onClick={() => overlay.mutate("mock_output")}
              size="sm"
              variant="ghost"
            >
              {t("studio.runtime.mock")}
            </Button>
            <Button
              aria-label={t("studio.runtime.removeOverlay")}
              disabled={removeOverlay.isPending || data.editorKind !== "action"}
              onClick={() => removeOverlay.mutate()}
              size="icon"
              variant="ghost"
            >
              <Trash2 className="size-3.5" />
            </Button>
          </div>
          <textarea
            aria-label={t("studio.details.output")}
            className="min-h-[260px] w-full resize-y rounded-md border border-border bg-canvas p-3 font-mono text-[11px] leading-5"
            onChange={(event) => setOverlayText(event.target.value)}
            value={overlayText}
          />
        </TabsContent>
        <TabsContent className="min-h-0 flex-1 overflow-auto" value="trace">
          {executionId ? <TraceDetail executionId={executionId} span={selectedSpan} /> : <p className="p-4 text-xs text-muted-foreground">{t("studio.runtime.noExecution")}</p>}
        </TabsContent>
      </Tabs>
      <div className="shrink-0 border-t border-border p-4">
        <Button className="w-full" onClick={onDelete} variant="secondary">
          <Trash2 className="size-4" />
          {t("studio.inspector.delete")}
        </Button>
      </div>
    </aside>
  );
}

function Parameters({
  data,
  manifest,
  localized,
  resources,
  workflowId,
  providerOptions,
  referenceCatalog,
  currentNodeCatalog,
  fieldErrors,
  onChange,
  t,
  onResourceAuthorize,
  onResourceRequest,
  sourceNodeId,
}: {
  data: InspectableNodeData;
  manifest?: NodeManifest;
  localized?: ReturnType<typeof localizeManifest>;
  resources: Partial<Record<ResourceType, ResourceOption[]>>;
  workflowId?: string;
  providerOptions: Record<string, ResourceOption[]>;
  referenceCatalog?: ReferenceCatalog;
  currentNodeCatalog?: ReferenceCatalog;
  fieldErrors: Record<string, string>;
  onChange: (data: Partial<ActionNodeData> | Partial<BindingNodeData>) => void;
  t: (key: string, fallback?: string) => string;
  onResourceAuthorize?: (option: ResourceOption) => Promise<void>;
  onResourceRequest?: (option: ResourceOption, context?: ResourceRequestContext) => Promise<void>;
  sourceNodeId?: string;
}) {
  const [advancedOpen, setAdvancedOpen] = useState(true);
  if (data.editorKind === "binding")
    return (
      <div className="space-y-4">
        <Field label={t("studio.inspector.attachment")}>
          <Input disabled value={t(`resourceGrants.resourceTypes.${data.resourceType}`)} />
        </Field>
        <ResourceSelect
          fieldPath="resourceId"
          label={t("studio.inspector.resource")}
          onChange={(resourceId, versionId, resourceName) =>
            onChange({ resourceId, resourceVersionId: versionId, resourceName })
          }
          options={resourceOptionsFor(resources, data.resourceType, data.operation)}
          onAuthorize={onResourceAuthorize}
          onRequest={onResourceRequest}
          sourceNodeId={sourceNodeId}
          testId="attachment-resource"
          value={data.resourceId}
        />
      </div>
    );
  return (
    <div className="space-y-5">
      <Field fieldPath="name" label={t("studio.inspector.name")}>
        <Input
          onChange={(event) => onChange({ label: event.target.value })}
          value={data.label}
        />
      </Field>
      <Field fieldPath="key" label={t("studio.inspector.key")}>
        <Input
          className="font-mono"
          onChange={(event) =>
            onChange({
              key: event.target.value.toLowerCase().replace(/[^a-z0-9_]/g, "_"),
            })
          }
          value={data.key}
        />
      </Field>
      {parameterEntries(manifest).filter(([name]) => data.nodeType !== "agent" || name === "systemPrompt" || name === "userQuestion").map(([name, schema]) => (
        <div data-field-path={`parameters.${name}`} key={name}>
          <ParameterField
            error={fieldErrors[`parameters.${name}`]}
            name={name}
            onChange={(value) =>
              onChange({ parameters: { ...data.parameters, [name]: value } })
            }
            parameters={data.parameters}
            providerOptions={providerOptions[name]}
            referenceCatalog={referenceCatalog}
            required={manifest?.parameterSchema.required?.includes(name)}
            schema={schema}
            ui={uiFields(manifest)[name]}
            value={data.parameters[name] ?? schema.default}
            workflowId={workflowId}
            labelOverride={localized?.parameterLabel(name)}
            descriptionOverride={localized?.parameterDescription(name)}
            enumLabels={Object.fromEntries(
              (schema.enum ?? []).map((option) => [
                String(option),
                localized?.parameterEnumLabel(name, String(option)) ?? String(option),
              ]),
            )}
            nestedLocalization={localized ? {
              label: localized.parameterLabel,
              description: localized.parameterDescription,
              placeholder: localized.parameterPlaceholder,
              enumLabel: localized.parameterEnumLabel,
            } : undefined}
          />
        </div>
      ))}
      {data.nodeType === "agent" && parameterEntries(manifest).some(([name]) => name !== "systemPrompt" && name !== "userQuestion") && (
        <section className="rounded-md border border-border/70">
          <div className="flex items-center gap-2 px-3 py-3">
            <Settings2 className="size-3.5 text-primary" />
            <div className="min-w-0 flex-1"><div className="text-xs font-semibold">{t("studio.inspector.advancedConfiguration")}</div><div className="mt-0.5 text-[10px] text-muted-foreground">{t("studio.inspector.advancedDescription")}</div></div>
            <Button aria-label={t("studio.inspector.toggleAdvanced")} onClick={() => setAdvancedOpen((value) => !value)} size="icon" variant="ghost">{advancedOpen ? <ChevronUp className="size-3.5" /> : <ChevronDown className="size-3.5" />}</Button>
          </div>
          {advancedOpen && <div className="space-y-4 border-t border-border p-3">
            {parameterEntries(manifest).filter(([name]) => name !== "systemPrompt" && name !== "userQuestion").map(([name, schema]) => (
              <div data-field-path={`parameters.${name}`} key={name}>
                <ParameterField
                  error={fieldErrors[`parameters.${name}`]}
                  name={name}
                  onChange={(value) => onChange({ parameters: { ...data.parameters, [name]: value } })}
                  parameters={data.parameters}
                  providerOptions={providerOptions[name]}
                  referenceCatalog={referenceCatalog}
                  required={manifest?.parameterSchema.required?.includes(name)}
                  schema={schema}
                  ui={uiFields(manifest)[name]}
                  value={data.parameters[name] ?? schema.default}
                  workflowId={workflowId}
                  labelOverride={localized?.parameterLabel(name)}
                  descriptionOverride={localized?.parameterDescription(name)}
                  enumLabels={Object.fromEntries((schema.enum ?? []).map((option) => [String(option), localized?.parameterEnumLabel(name, String(option)) ?? String(option)]))}
                  nestedLocalization={localized ? { label: localized.parameterLabel, description: localized.parameterDescription, placeholder: localized.parameterPlaceholder, enumLabel: localized.parameterEnumLabel } : undefined}
                />
              </div>
            ))}
          </div>}
        </section>
      )}
      {resourceSelectors(manifest).map((selector) => (
        <ResourceSelect
          fieldPath="resourceReferences"
          key={`${selector.resourceType}-${selector.operation}`}
          label={selector.label ?? selector.resourceType.replaceAll("_", " ")}
          onChange={(resourceId, versionId) =>
            onChange({
              resourceReferences: [
                ...data.resourceReferences.filter(
                  (reference) =>
                    reference.bindingId ||
                    reference.resourceType !== selector.resourceType,
                ),
                {
                  resourceType: selector.resourceType,
                  resourceId,
                  resourceVersionId: versionId,
                  operation: selector.operation,
                },
              ],
            })
          }
          options={resourceOptionsFor(resources, selector.resourceType, selector.operation)}
          onAuthorize={onResourceAuthorize}
          onRequest={onResourceRequest}
          sourceNodeId={sourceNodeId}
          required={selector.required}
          optionsMissing={Boolean(
            selector.required &&
            !resourceOptionsFor(resources, selector.resourceType, selector.operation).length,
          )}
          testId={`resource-selector-${selector.resourceType}`}
          value={
            data.resourceReferences.find(
              (reference) =>
                !reference.bindingId &&
                reference.resourceType === selector.resourceType,
            )?.resourceId
          }
        />
      ))}
      {Boolean(manifest?.outputProjectionSchema) && (
        <ProjectionConfiguration
          label={t("studio.inspector.outputProjection")}
          manifest={manifest!}
          onChange={(outputProjection) => onChange({ outputProjection: outputProjection as ActionNodeData["outputProjection"] })}
          value={data.outputProjection}
          referenceCatalog={currentNodeCatalog}
        />
      )}
      {manifest?.contextWriteCapability && (
        <ContextWritesConfiguration
          label={t("studio.inspector.contextWrites")}
          onChange={(contextWrites) =>
            onChange({
              contextWrites: (Array.isArray(contextWrites) ? contextWrites : []) as ActionNodeData["contextWrites"],
            })
          }
          value={data.contextWrites}
          referenceCatalog={currentNodeCatalog}
        />
      )}
      <Field
        fieldPath="settings.onError"
        label={t("studio.inspector.errorPolicy")}
      >
        <Select
          className="w-full"
          onValueChange={(onError) =>
            onChange({ settings: { ...data.settings, onError } })
          }
          options={[
            {
              value: "stop",
              label: t("studio.inspector.errorStop"),
            },
            {
              value: "continue_error_output",
              label: t("studio.inspector.errorOutput"),
            },
          ]}
          value={String(data.settings.onError ?? "stop")}
        />
        {fieldErrors["settings.onError"] && (
          <span className="mt-1 block text-[10px] text-danger">
            {fieldErrors["settings.onError"]}
          </span>
        )}
      </Field>
      {manifest?.bindingSlots.length ? (
        <div className="border-t border-border pt-4">
          <div className="mb-2 flex items-center gap-2 text-xs font-semibold">
            <Settings2 className="size-3.5 text-primary" />
            {t("studio.inspector.bindings")}
          </div>
          {manifest.bindingSlots.map((slot) => (
            <div
              className="flex items-center justify-between py-1.5 text-[11px]"
              data-field-path={`resourceReferences.${slot.name}`}
              key={slot.name}
            >
              <span>{localized?.bindingSlotLabel(slot.name) ?? slot.name}</span>
              <span
                className={
                  slot.required ? "text-warning" : "text-muted-foreground"
                }
              >
                {t(
                  slot.required
                    ? "studio.inspector.required"
                    : slot.multiple
                      ? "studio.inspector.multiple"
                      : "studio.inspector.optional",
                )}
              </span>
            </div>
          ))}
        </div>
      ) : null}
    </div>
  );
}

function CompositeUpgrade({
  current,
  versions,
  parameters,
  onChange,
}: {
  current: NodeManifest;
  versions: NodeManifest[];
  parameters: Record<string, unknown>;
  onChange: (data: Partial<ActionNodeData>) => void;
}) {
  const { t } = useTranslation();
  const [candidateType, setCandidateType] = useState("");
  const candidate = versions.find((item) => item.nodeType === candidateType);
  return (
    <section className="mb-5 border-b border-border pb-4">
      <div className="text-xs font-semibold">{t('studio.inspector.workflowVersion')}</div>
      <div className="mt-2 flex gap-2">
        <Select
          className="min-w-0 flex-1"
          onValueChange={setCandidateType}
          options={versions.map((item) => ({
            value: item.nodeType,
            label: `v${item.parameterSchema["x-agentx-versionNumber"] ?? "?"}`,
          }))}
          placeholder={`v${current.parameterSchema["x-agentx-versionNumber"] ?? "?"}`}
          value={candidateType}
        />
        <Button
          disabled={!candidate}
          onClick={() =>
            candidate &&
            onChange({
              nodeType: candidate.nodeType,
              typeVersion: candidate.version,
              parameters: {
                ...parameters,
                workflowVersionId:
                  candidate.parameterSchema["x-agentx-workflowVersionId"],
              },
            })
          }
          size="sm"
        >
          Upgrade
        </Button>
      </div>
      {candidate && <ContractDiff current={current} next={candidate} />}
    </section>
  );
}

function ContractDiff({
  current,
  next,
}: {
  current: NodeManifest;
  next: NodeManifest;
}) {
  const rows = (["inputs", "outputs", "contexts"] as const).map((kind) => {
    const before = contractKeys(current, kind);
    const after = contractKeys(next, kind);
    return {
      kind,
      added: after.filter((key) => !before.includes(key)),
      removed: before.filter((key) => !after.includes(key)),
    };
  });
  return (
    <div className="mt-3 space-y-1 text-[10px]">
      {rows.map((row) => (
        <div className="grid grid-cols-[64px_1fr] gap-2" key={row.kind}>
          <span className="text-muted-foreground">{row.kind}</span>
          <span>
            <span className="text-success">
              + {row.added.join(", ") || "none"}
            </span>
            <span className="ml-2 text-danger">
              - {row.removed.join(", ") || "none"}
            </span>
          </span>
        </div>
      ))}
    </div>
  );
}

function contractKeys(
  manifest: NodeManifest,
  kind: "inputs" | "outputs" | "contexts",
) {
  if (kind === "outputs")
    return Object.keys(
      (
        manifest.outputSchema as
          { properties?: Record<string, unknown> } | undefined
      )?.properties ?? {},
    );
  if (kind === "contexts")
    return Object.keys(
      manifest.parameterSchema["x-agentx-contextContract"] ?? {},
    );
  const input = manifest.parameterSchema.properties?.inputs as
    JsonSchemaWithAllOf | undefined;
  return Object.keys(input?.allOf?.[0]?.properties ?? input?.properties ?? {});
}

type JsonSchemaWithAllOf = {
  properties?: Record<string, unknown>;
  allOf?: Array<{ properties?: Record<string, unknown> }>;
};

function ProjectionConfiguration({
  label,
  manifest,
  value,
  onChange,
  referenceCatalog,
}: {
  label: string;
  manifest: NodeManifest;
  value: unknown;
  onChange: (value: unknown) => void;
  referenceCatalog?: ReferenceCatalog;
}) {
  const { t } = useTranslation();
  const projection = value && typeof value === "object" && !Array.isArray(value) ? value as ActionNodeData["outputProjection"] : {};
  const ports = manifest.outputPorts.filter((port) => port.kind !== "error").map((port) => port.name);
  const [dialogOpen, setDialogOpen] = useState(false);
  const [draft, setDraft] = useState<ProjectionDraft | null>(null);
  const add = () => {
    const port = ports[0] ?? "main";
    const fields = projection[port] ?? {};
    let index = Object.keys(fields).length + 1;
    while (fields[`custom_${index}`]) index += 1;
    setDraft({ port, name: `custom_${index}`, field: { expression: "", schema: { type: "string" }, sensitive: false } });
    setDialogOpen(true);
  };
  const edit = (port: string, name: string) => {
    setDraft({ port, name, originalPort: port, originalName: name, field: structuredClone(projection[port][name]) });
    setDialogOpen(true);
  };
  const save = () => {
    if (!draft?.name.trim()) return;
    const clean = draft.name.trim();
    const next = { ...projection };
    if (draft.originalName && draft.originalPort) {
      const originalFields = { ...next[draft.originalPort] };
      delete originalFields[draft.originalName];
      next[draft.originalPort] = originalFields;
    }
    next[draft.port] = { ...next[draft.port], [clean]: draft.field };
    onChange(next);
    setDialogOpen(false);
    setDraft(null);
  };
  const remove = (port: string, name: string) => {
    const next = { ...projection, [port]: { ...projection[port] } };
    delete next[port][name];
    onChange(next);
  };
  const nativeFields = Object.entries((manifest.outputSchema as { properties?: Record<string, { type?: string }> } | undefined)?.properties ?? {});
  return <section><div className="mb-2 flex items-center justify-between gap-3"><span className="text-xs text-muted-foreground">{label}</span><Button onClick={add} size="sm" variant="secondary"><Plus className="size-3.5" />{t("studio.inspector.addProjection")}</Button></div><div className="space-y-2">{nativeFields.length > 0 && <div className="rounded-md bg-muted/40 p-2"><div className="mb-1 text-[10px] font-medium text-muted-foreground">{t("studio.inspector.nativeOutputs")}</div>{nativeFields.map(([name, schema]) => <div className="flex justify-between py-1 text-[11px]" key={name}><code>{name}</code><span className="text-muted-foreground">{schema.type ?? "unknown"}</span></div>)}</div>}{Object.entries(projection).flatMap(([port, fields]) => Object.entries(fields).map(([name, field]) => <div className="flex items-center gap-3 rounded-md border border-border px-3 py-2.5" key={`${port}:${name}`}><div className="min-w-0 flex-1"><div className="flex items-center gap-2"><code className="truncate text-xs font-medium">{name}</code><span className="rounded bg-muted px-1.5 py-0.5 text-[10px] text-muted-foreground">{projectionType(field.schema)}</span>{field.sensitive && <span className="text-[10px] text-warning">{t("studio.interface.sensitive")}</span>}</div><div className="truncate text-[10px] text-muted-foreground">{port} · {field.expression || t("studio.inspector.noExpression")}</div></div><Button aria-label={t("studio.editField")} onClick={() => edit(port, name)} size="icon" variant="ghost"><Settings2 className="size-3.5" /></Button><Button aria-label={t("studio.removeField")} onClick={() => remove(port, name)} size="icon" variant="ghost"><Trash2 className="size-3.5" /></Button></div>))}</div><ProjectionDialog draft={draft} onChange={setDraft} onClose={() => { setDialogOpen(false); setDraft(null); }} onSave={save} open={dialogOpen} ports={ports} referenceCatalog={referenceCatalog} /></section>;
}

type ProjectionDraft = {
  port: string;
  name: string;
  originalPort?: string;
  originalName?: string;
  field: ActionNodeData["outputProjection"][string][string];
};

function ProjectionDialog({ open, draft, ports, referenceCatalog, onChange, onClose, onSave }: { open: boolean; draft: ProjectionDraft | null; ports: string[]; referenceCatalog?: ReferenceCatalog; onChange: (value: ProjectionDraft | null) => void; onClose: () => void; onSave: () => void }) {
  const { t } = useTranslation();
  if (!draft) return null;
  const setField = (patch: Partial<ProjectionDraft["field"]>) => onChange({ ...draft, field: { ...draft.field, ...patch } });
  return <Dialog onOpenChange={(value) => !value && onClose()} open={open}><DialogContent description={t("studio.inspector.projectionDialogDescription")} title={draft.originalName ? t("studio.inspector.editProjection") : t("studio.inspector.addProjection")}><div className="space-y-4 p-5"><div><h2 className="text-sm font-semibold">{draft.originalName ? t("studio.inspector.editProjection") : t("studio.inspector.addProjection")}</h2><p className="mt-1 text-[11px] text-muted-foreground">{t("studio.inspector.projectionDialogDescription")}</p></div><div className="grid gap-3">{ports.length > 1 && <label className="text-xs"><span className="mb-1 block text-muted-foreground">{t("studio.inspector.outputPort")}</span><Select onValueChange={(port) => onChange({ ...draft, port })} options={ports.map((port) => ({ value: port, label: port }))} value={draft.port} /></label>}<label className="text-xs"><span className="mb-1 block text-muted-foreground">{t("studio.interface.fieldType")}</span><Select aria-label={t("studio.interface.fieldType")} onValueChange={(type) => setField({ schema: { type } })} options={schemaTypeOptions(t)} value={projectionType(draft.field.schema)} /></label><label className="text-xs"><span className="mb-1 block text-muted-foreground">{t("studio.interface.outputName")}</span><Input aria-label={t("studio.interface.outputName")} onChange={(event) => onChange({ ...draft, name: event.target.value })} value={draft.name} /></label><label className="text-xs"><span className="mb-1 block text-muted-foreground">{t("studio.interface.content")}</span><ReferenceTextInput allowed={["inputs", "outputs", "contexts", "item"]} catalog={referenceCatalog} expectedType={projectionType(draft.field.schema)} onChange={(expression) => setField({ expression })} placeholder="${{ item.json.value }}" value={draft.field.expression} /></label><label className="flex items-center gap-2 text-xs"><input checked={draft.field.sensitive} className="size-4 accent-primary" onChange={(event) => setField({ sensitive: event.target.checked })} type="checkbox" />{t("studio.interface.sensitive")}</label></div><div className="flex justify-end gap-2 border-t border-border pt-4"><Button onClick={onClose} variant="ghost">{t("common.cancel")}</Button><Button disabled={!draft.name.trim()} onClick={onSave}>{t("common.save")}</Button></div></div></DialogContent></Dialog>;
}

function schemaTypeOptions(t: ReturnType<typeof useTranslation>["t"]) {
  return ["string", "number", "boolean", "object", "array"].map((value) => ({
    value,
    label: t(`studio.schemaTypes.${value}`, value),
  }));
}

function projectionType(schema: unknown) {
  return schema && typeof schema === "object" && !Array.isArray(schema)
    ? String((schema as { type?: unknown }).type ?? "string")
    : "string";
}

function ReferenceTextInput({ allowed, catalog, expectedType, value, placeholder, onChange }: { allowed: Array<"inputs" | "outputs" | "contexts" | "item">; catalog?: ReferenceCatalog; expectedType?: string; value: string; placeholder: string; onChange: (value: string) => void }) {
  const { t } = useTranslation();
  const input = useRef<HTMLInputElement>(null);
  const insert = (reference: string) => {
    const result = insertAtSelection(value, reference, input.current?.selectionStart, input.current?.selectionEnd);
    onChange(result.value);
    requestAnimationFrame(() => { input.current?.focus(); input.current?.setSelectionRange(result.cursor, result.cursor); });
  };
  return <ReferenceControl allowed={allowed} catalog={catalog} enabled={Boolean(catalog)} expectedType={expectedType} onInsert={insert}><Input aria-label={t("studio.expression")} className="font-mono text-[11px]" onChange={(event) => onChange(event.target.value)} placeholder={placeholder} ref={input} value={value} /></ReferenceControl>;
}

function addCurrentNodeReferences(
  catalog: ReferenceCatalog,
  data: ActionNodeData,
  manifest: NodeManifest,
  currentRawOutputLabel: string,
): ReferenceCatalog {
  const nativeSchema = asReferenceSchema(manifest.outputSchema);
  const itemPath = "item.json";
  const itemFields = referenceFields(nativeSchema, itemPath);
  const nodePath = `outputs.${data.key}`;
  const ports = manifest.outputPorts.map((port) => {
    const portNativeSchema = asReferenceSchema(
      manifest.outputPortSchemas?.[port.name] ?? manifest.outputSchema,
    );
    const projected = Object.fromEntries(
      Object.entries(data.outputProjection[port.name] ?? {}).map(([name, field]) => [
        name,
        asReferenceSchema(field.schema),
      ]),
    );
    const schema = {
      ...portNativeSchema,
      properties: { ...portNativeSchema.properties, ...projected },
    };
    const jsonPath = `${nodePath}.${port.name}.current.json`;
    const fields = referenceFields(schema, jsonPath);
    const current: ReferenceEntry = {
      id: jsonPath,
      label: "current",
      path: jsonPath,
      expression: fields.length ? undefined : `\${{ ${jsonPath} }}`,
      type: "object",
      nullable: true,
      children: fields,
    };
    return {
      id: `${nodePath}.${port.name}`,
      label: port.name,
      path: `${nodePath}.${port.name}`,
      children: [current],
    } satisfies ReferenceEntry;
  });
  return {
    ...catalog,
    outputs: [
      ...catalog.outputs,
      { id: nodePath, label: data.key, path: nodePath, children: ports },
    ],
    item: [
      {
        id: itemPath,
        label: currentRawOutputLabel,
        path: itemPath,
        expression: itemFields.length ? undefined : `\${{ ${itemPath} }}`,
        type: "object",
        children: itemFields,
      },
    ],
  };
}

type ReferenceSchema = {
  type?: string;
  properties?: Record<string, ReferenceSchema>;
  required?: string[];
};

function asReferenceSchema(value: unknown): ReferenceSchema {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as ReferenceSchema)
    : {};
}

function referenceFields(schema: ReferenceSchema, parent: string): ReferenceEntry[] {
  return Object.entries(schema.properties ?? {}).map(([name, child]) => {
    const path = `${parent}.${name}`;
    const children = referenceFields(child, path);
    return {
      id: path,
      label: name,
      path,
      expression: children.length ? undefined : `\${{ ${path} }}`,
      type: child.type,
      nullable: !(schema.required ?? []).includes(name),
      children,
    };
  });
}

function ContextWritesConfiguration({ label, value, onChange, referenceCatalog }: { label: string; value: unknown; onChange: (value: unknown) => void; referenceCatalog?: ReferenceCatalog }) {
  const { t } = useTranslation();
  const writes = Array.isArray(value) ? value as Array<{ operation?: string; path?: string; value?: unknown }> : [];
  const [dialogOpen, setDialogOpen] = useState(false);
  const [editingIndex, setEditingIndex] = useState<number | null>(null);
  const [draft, setDraft] = useState<ContextWriteDraft>({ operation: "set", path: "", value: "" });
  const add = () => { setEditingIndex(null); setDraft({ operation: "set", path: "", value: "" }); setDialogOpen(true); };
  const edit = (index: number) => { setEditingIndex(index); setDraft({ operation: writes[index].operation ?? "set", path: writes[index].path ?? "", value: writes[index].value ?? "" }); setDialogOpen(true); };
  const save = () => {
    onChange(editingIndex === null ? [...writes, draft] : writes.map((write, index) => index === editingIndex ? draft : write));
    setDialogOpen(false);
  };
  return <section><div className="mb-2 flex items-center justify-between gap-3"><span className="text-xs text-muted-foreground">{label}</span><Button onClick={add} size="sm" variant="secondary"><Plus className="size-3.5" />{t("studio.inspector.addContextWrite")}</Button></div><div className="space-y-2">{writes.map((write, index) => <div className="flex items-center gap-3 rounded-md border border-border px-3 py-2.5" key={`${write.path ?? "context"}-${index}`}><div className="min-w-0 flex-1"><div className="flex items-center gap-2"><code className="truncate text-xs font-medium">{write.path || t("studio.inspector.contextPath")}</code><span className="rounded bg-muted px-1.5 py-0.5 text-[10px] text-muted-foreground">{t(`studio.contextOperations.${write.operation ?? "set"}`, write.operation ?? "set")}</span></div>{write.operation !== "delete" && <div className="truncate text-[10px] text-muted-foreground">{typeof write.value === "string" ? write.value : JSON.stringify(write.value ?? "")}</div>}</div><Button aria-label={t("studio.inspector.editContextWrite")} onClick={() => edit(index)} size="icon" variant="ghost"><Settings2 className="size-3.5" /></Button><Button aria-label={t("studio.removeField")} onClick={() => onChange(writes.filter((_, current) => current !== index))} size="icon" variant="ghost"><Trash2 className="size-3.5" /></Button></div>)}</div><ContextWriteDialog draft={draft} onChange={setDraft} onClose={() => setDialogOpen(false)} onSave={save} open={dialogOpen} referenceCatalog={referenceCatalog} editing={editingIndex !== null} /></section>;
}

type ContextWriteDraft = { operation: string; path: string; value: unknown };

function ContextWriteDialog({ open, editing, draft, referenceCatalog, onChange, onClose, onSave }: { open: boolean; editing: boolean; draft: ContextWriteDraft; referenceCatalog?: ReferenceCatalog; onChange: (value: ContextWriteDraft) => void; onClose: () => void; onSave: () => void }) {
  const { t } = useTranslation();
  const value = typeof draft.value === "string" ? draft.value : JSON.stringify(draft.value ?? "");
  const variableOptions = globalVariableOptions(referenceCatalog?.contexts ?? []);
  const selectVariable = (path: string) => {
    const operations = contextOperationsFor(referenceCatalog, path);
    onChange({ ...draft, path, operation: operations.includes(draft.operation) ? draft.operation : "set" });
  };
  return <Dialog onOpenChange={(next) => !next && onClose()} open={open}><DialogContent description={t("studio.inspector.contextWriteDialogDescription")} title={editing ? t("studio.inspector.editContextWrite") : t("studio.inspector.addContextWrite")}><div className="space-y-4 p-5"><div><h2 className="text-sm font-semibold">{editing ? t("studio.inspector.editContextWrite") : t("studio.inspector.addContextWrite")}</h2><p className="mt-1 text-[11px] text-muted-foreground">{t("studio.inspector.contextWriteDialogDescription")}</p></div><div className="grid gap-3"><label className="text-xs"><span className="mb-1 block text-muted-foreground">{t("studio.inspector.contextPath")}</span><Select aria-label={t("studio.inspector.contextPath")} disabled={variableOptions.length === 0} onValueChange={selectVariable} options={variableOptions} placeholder={t("studio.inspector.selectContext")} value={draft.path} /></label><label className="text-xs"><span className="mb-1 block text-muted-foreground">{t("studio.inspector.contextOperation")}</span><Select aria-label={t("studio.inspector.contextOperation")} disabled={!draft.path} onValueChange={(operation) => onChange({ ...draft, operation })} options={contextOperationsFor(referenceCatalog, draft.path).map((operation) => ({ value: operation, label: t(`studio.contextOperations.${operation}`, operation) }))} value={draft.operation} /></label><label className="text-xs"><span className="mb-1 block text-muted-foreground">{t("studio.inspector.contextValue")}</span>{draft.operation === "delete" ? <Input aria-label={t("studio.inspector.contextValue")} disabled value="" /> : <ReferenceTextInput allowed={["inputs", "outputs", "contexts"]} catalog={referenceCatalog} onChange={(next) => onChange({ ...draft, value: next })} placeholder="${{ outputs.node.main.current.json }}" value={value} />}</label></div><div className="flex justify-end gap-2 border-t border-border pt-4"><Button onClick={onClose} variant="ghost">{t("common.cancel")}</Button><Button disabled={!draft.path.trim()} onClick={onSave}>{t("common.save")}</Button></div></div></DialogContent></Dialog>;
}

function globalVariableOptions(entries: ReferenceEntry[]): Array<{ value: string; label: string }> {
  return entries.flatMap((entry) => {
    const value = entry.path.replace(/^contexts\./, "");
    return [{ value, label: value }, ...globalVariableOptions(entry.children)];
  });
}

function contextOperationsFor(catalog: ReferenceCatalog | undefined, path: string | undefined) {
  const common = ["set", "set_if_absent", "compare_and_set", "delete"];
  const target = findReference(catalog?.contexts ?? [], `contexts.${path ?? ""}`);
  if (target?.type === "array") return [...common.slice(0, 2), "append", ...common.slice(2)];
  if (target?.type === "object") return [...common.slice(0, 2), "merge_object", ...common.slice(2)];
  if (target?.type === "number" || target?.type === "integer") return [...common.slice(0, 2), "increment", "min", "max", ...common.slice(2)];
  return common;
}

function findReference(entries: ReferenceEntry[], path: string): ReferenceEntry | undefined {
  for (const entry of entries) {
    if (entry.path === path) return entry;
    const nested = findReference(entry.children, path);
    if (nested) return nested;
  }
  return undefined;
}

function ResourceSelect({
  label,
  options,
  value,
  required,
  optionsMissing,
  testId,
  fieldPath,
  onChange,
  onAuthorize,
  onRequest,
  sourceNodeId,
}: {
  label: string;
  options: ResourceOption[];
  value?: string;
  required?: boolean;
  optionsMissing?: boolean;
  testId?: string;
  fieldPath?: string;
  onChange: (id: string, versionId?: string | null, label?: string) => void;
  onAuthorize?: (option: ResourceOption) => Promise<void>;
  onRequest?: (option: ResourceOption, context?: ResourceRequestContext) => Promise<void>;
  sourceNodeId?: string;
}) {
  const { t } = useTranslation();
  return (
    <Field
      fieldPath={fieldPath}
      label={label}
      required={required}
      testId={testId}
    >
      <ResourcePicker onAuthorize={onAuthorize} onChange={onChange} onRequest={onRequest ? (option, message) => onRequest(option, { sourceNodeId, message }) : undefined} options={options} value={value} />
      {optionsMissing && (
        <span className="mt-1 block text-[10px] text-warning">
          {t("studio.inspector.noResource")}
        </span>
      )}
    </Field>
  );
}
function Field({
  children,
  label,
  required,
  testId,
  fieldPath,
}: {
  children: React.ReactNode;
  label: string;
  required?: boolean;
  testId?: string;
  fieldPath?: string;
}) {
  return (
    <label
      className="block text-xs"
      data-field-path={fieldPath}
      data-testid={testId}
    >
      <span className="mb-1.5 block text-muted-foreground">
        {label}
        {required && <span className="ml-1 text-danger">*</span>}
      </span>
      {children}
    </label>
  );
}
function JsonValue({ value }: { value: unknown }) {
  return (
    <pre className="whitespace-pre-wrap break-all font-mono text-[11px] leading-5 text-muted-foreground">
      {JSON.stringify(value ?? {}, null, 2)}
    </pre>
  );
}
type ResourceSelector = {
  resourceType: ResourceType;
  operation: "view" | "use" | "read" | "write" | "manage";
  required?: boolean;
  label?: string;
};
function resourceSelectors(manifest?: NodeManifest): ResourceSelector[] {
  const value = manifest?.uiSchema.resourceSelectors;
  if (!Array.isArray(value)) return [];
  return value.filter((item): item is ResourceSelector =>
    Boolean(
      item &&
      typeof item === "object" &&
      typeof (item as ResourceSelector).resourceType === "string" &&
      typeof (item as ResourceSelector).operation === "string",
    ),
  );
}
function resourceOptionsFor(
  resources: Partial<Record<ResourceType, ResourceOption[]>>,
  resourceType: ResourceType,
  operation: ResourceOption["operation"],
) {
  return (resources[resourceType] ?? []).filter(
    (option) => option.operation === undefined || option.operation === operation,
  );
}
function parameterEntries(manifest?: NodeManifest) {
  return Object.entries(manifest?.parameterSchema.properties ?? {});
}
function uiFields(manifest?: NodeManifest): Record<string, UiField> {
  return manifest?.uiSchema.fields ?? {};
}
