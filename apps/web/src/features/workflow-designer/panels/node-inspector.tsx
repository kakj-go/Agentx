import { useMutation, useQuery } from "@tanstack/react-query";
import { MoreHorizontal, Play, Power, Trash2, X } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { apiRequest } from "../../../shared/api/client";
import { Button } from "../../../shared/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "../../../shared/ui/dropdown-menu";
import { Input } from "../../../shared/ui/input";
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
import { TraceNodeDetails } from "../../traces/trace-node-card";
import { useExecutionArtifactDownload } from "../../traces/use-execution-artifact-download";
import type {
  ActionNodeData,
  JsonSchemaProperty,
  NodeManifest,
  ReferenceCatalog,
  ReferenceEntry,
  ValueSelector,
  ResourceOption,
  ResourceRequestContext,
  ResourceType,
  UiField,
} from "../model/types";
import {
  localizeManifest,
  localizedNodeLabel,
} from "../model/manifest-localization";
import { SUPPORTED_CONTROLS } from "../forms/parameter-field";
import { isReferenceKey } from "../utils/definition-validation";
import { InspectorShell } from "./inspector-shell";
import { nodeGroupColor } from "../nodes/node-appearance";
import { NodeIcon } from "../nodes/node-icon";
import { Badge } from "../../../shared/ui/badge";
import { AgentPanel } from "./inspectors/agent-panel";
import { ApprovalPanel } from "./inspectors/approval-panel";
import { CodePanel } from "./inspectors/code-panel";
import { HttpPanel } from "./inspectors/http-panel";
import { IfPanel } from "./inspectors/if-panel";
import { ListPanel } from "./inspectors/list-panel";
import { LoopPanel } from "./inspectors/loop-panel";
import { MergePanel } from "./inspectors/merge-panel";
import { ModelPanel } from "./inspectors/model-panel";
import { SetPanel } from "./inspectors/set-panel";
import { SubworkflowPanel } from "./inspectors/subworkflow-panel";
import {
  PanelField,
  type ActionPanelProps,
} from "./inspectors/panel-shell";

const ACTION_PANELS: Partial<Record<string, (panel: ActionPanelProps) => React.ReactNode>> = {
  agent: AgentPanel,
  approval: ApprovalPanel,
  code: CodePanel,
  declarative_http: HttpPanel,
  if: IfPanel,
  list: ListPanel,
  loop_over_items: LoopPanel,
  merge: MergePanel,
  model: ModelPanel,
  set: SetPanel,
  sub_workflow: SubworkflowPanel,
};

export function NodeInspector({
  data,
  nodeId,
  manifest,
  resources,
  workflowId,
  executionId,
  referenceCatalog,
  parameterCatalogs,
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
  data?: ActionNodeData;
  nodeId?: string;
  manifest?: NodeManifest;
  resources: Partial<Record<ResourceType, ResourceOption[]>>;
  workflowId?: string;
  executionId?: string;
  referenceCatalog?: ReferenceCatalog;
  parameterCatalogs?: Record<string, ReferenceCatalog | undefined>;
  open?: boolean;
  onClose?: () => void;
  onRun?: () => void;
  onChange: (data: Partial<ActionNodeData>) => void;
  onDelete: () => void;
  onOverlayChange?: (nodeId: string, overlayId?: string) => void;
  fieldErrors?: Record<string, string>;
  onValidityChange?: (valid: boolean) => void;
  onResourceAuthorize?: (option: ResourceOption) => Promise<void>;
  onResourceRequest?: (option: ResourceOption, context?: ResourceRequestContext) => Promise<void>;
}) {
  const { t, i18n } = useTranslation();
  const downloadArtifact = useExecutionArtifactDownload(executionId);
  const [tab, setTab] = useState("parameters");
  const providerOptions = useProviderOptions(manifest);
  const resolvedManifest = data?.nodeType === "sub_workflow"
    ? providerOptions.workflowVersionId?.find((option) => option.value === data.parameters.workflowVersionId)?.manifest ?? manifest
    : manifest;
  const unsupported = parameterEntries(resolvedManifest).some(
    ([name]) =>
      !SUPPORTED_CONTROLS.has(uiFields(resolvedManifest)[name]?.control ?? ""),
  );
  const dataKey = data?.nodeType;
  const localized = resolvedManifest
    ? localizeManifest(resolvedManifest, i18n.language)
    : undefined;
  const title =
    data && resolvedManifest
      ? localizedNodeLabel(resolvedManifest, data.label, i18n.language)
      : (data?.label ?? "");
  const currentNodeCatalog = useMemo(
    () =>
      data?.editorKind === "action" && resolvedManifest && referenceCatalog
        ? addCurrentNodeReferences(referenceCatalog, data, resolvedManifest, t("studio.references.currentRawOutput"), nodeId)
        : referenceCatalog,
    [data, resolvedManifest, nodeId, referenceCatalog, t],
  );
  const parameterCatalog = useMemo(
    () => referenceCatalog
      ? addCurrentItemReference(referenceCatalog, t("studio.references.currentInput"))
      : referenceCatalog,
    [referenceCatalog, t],
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
    refetchInterval: executionId ? 2_000 : false,
  });
  const selectedRuns = useMemo(
    () => nodeRuns.data?.items.filter((node) => node.nodeId === nodeId) ?? [],
    [nodeId, nodeRuns.data?.items],
  );
  const selectedRun = selectedRuns.at(-1);
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
    setOverlayText("{}");
  }, [nodeId]);
  useEffect(() => {
    if (selectedRun?.output !== undefined)
      setOverlayText(JSON.stringify(selectedRun.output, null, 2));
  }, [selectedRun?.output]);
  useEffect(() => {
    if (data) setTab("parameters");
  }, [data, dataKey]);

  if (!open || !data) return null;
  return (
    <InspectorShell testId="node-details-view">
      <div className="relative z-[101] flex shrink-0 items-center gap-3 border-b border-border bg-surface px-4 py-3">
        <span className="grid size-9 place-items-center rounded-lg" style={{ color: nodeGroupColor(data.nodeType), backgroundColor: `color-mix(in srgb, ${nodeGroupColor(data.nodeType)} 12%, transparent)` }}>
          <NodeIcon
            className="size-5"
            iconKey={resolvedManifest?.iconKey ?? "box"}
          />
        </span>
        <div className="min-w-0 flex-1">
          <strong className="block truncate text-sm font-semibold">
            {title}
          </strong>
          <div className="flex min-w-0 items-center gap-2">
            <span className="block truncate text-[10px] text-muted-foreground">
              {localized?.displayName ?? data.nodeType}
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
          <Parameters
            data={data}
            fieldErrors={fieldErrors}
            localized={localized}
            manifest={resolvedManifest}
            onChange={onChange}
            providerOptions={providerOptions}
            referenceCatalog={parameterCatalog}
            parameterCatalogs={parameterCatalogs}
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
              disabled={overlay.isPending || data.editorKind !== "action" || selectedRun?.output === undefined}
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
          {executionId && selectedRun ? <TraceNodeDetails active={tab === "trace"} executionId={executionId} node={selectedRun} onDownloadArtifact={downloadArtifact} /> : <p className="p-4 text-xs text-muted-foreground">{executionId ? t("trace.noNodeRuns") : t("studio.runtime.noExecution")}</p>}
        </TabsContent>
      </Tabs>
      <div className="shrink-0 border-t border-border p-4">
        <Button className="w-full" onClick={onDelete} variant="secondary">
          <Trash2 className="size-4" />
          {t("studio.inspector.delete")}
        </Button>
      </div>
    </InspectorShell>
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
  parameterCatalogs,
  currentNodeCatalog,
  fieldErrors,
  onChange,
  t,
  onResourceAuthorize,
  onResourceRequest,
  sourceNodeId,
}: {
  data: ActionNodeData;
  manifest?: NodeManifest;
  localized?: ReturnType<typeof localizeManifest>;
  resources: Partial<Record<ResourceType, ResourceOption[]>>;
  workflowId?: string;
  providerOptions: Record<string, ResourceOption[]>;
  referenceCatalog?: ReferenceCatalog;
  parameterCatalogs?: Record<string, ReferenceCatalog | undefined>;
  currentNodeCatalog?: ReferenceCatalog;
  fieldErrors: Record<string, string>;
  onChange: (data: Partial<ActionNodeData>) => void;
  t: (key: string, fallback?: string) => string;
  onResourceAuthorize?: (option: ResourceOption) => Promise<void>;
  onResourceRequest?: (option: ResourceOption, context?: ResourceRequestContext) => Promise<void>;
  sourceNodeId?: string;
}) {
  const nameError = !data.label.trim() || data.label.length > 160 ? t("studio.validation.issues.INVALID_NODE_NAME") : undefined;
  const keyError = !isReferenceKey(data.key) ? t("studio.validation.issues.INVALID_NODE_KEY") : undefined;
  const panelContext = manifest
    ? {
        data,
        manifest,
        localized,
        fieldErrors,
        providerOptions,
        referenceCatalog,
        parameterCatalogs,
        currentNodeCatalog,
        resources,
        workflowId,
        sourceNodeId,
        onChange,
        onResourceAuthorize,
        onResourceRequest,
      }
    : undefined;
  const Panel =
    data.editorKind === "action" && panelContext
      ? ACTION_PANELS[data.nodeType]
      : undefined;
  return (
    <div className="space-y-5">
      <details className="rounded-md border border-border/60" data-testid="node-basics">
        <summary className="flex cursor-pointer items-center justify-between gap-2 px-2.5 py-2 text-[11px] text-muted-foreground">
          <span>{t("studio.inspector.basics")}</span>
          <span className="min-w-0 truncate font-medium text-foreground/70">{data.label}</span>
        </summary>
        <div className="space-y-3 border-t border-border/60 p-2.5">
          <PanelField error={fieldErrors.name ?? nameError} fieldPath="name" label={t("studio.inspector.name")} required>
            <Input
              onChange={(event) => onChange({ label: event.target.value })}
              value={data.label}
            />
          </PanelField>
          <PanelField error={fieldErrors.key ?? keyError} fieldPath="key" label={t("studio.inspector.key")} required>
            <Input
              className="font-mono"
              onChange={(event) =>
                onChange({
                  key: event.target.value.toLowerCase().replace(/[^a-z0-9_]/g, "_"),
                })
              }
              value={data.key}
            />
          </PanelField>
        </div>
      </details>
      {Panel && panelContext ? (
        <Panel {...panelContext} />
      ) : (
        <div className="rounded-md border border-danger/30 bg-danger/5 p-3 text-xs text-danger" role="alert">
          {t("studio.inspector.unsupportedNodePanel").replace("{{nodeType}}", data.nodeType)}
        </div>
      )}
    </div>
  );
}

function JsonValue({ value }: { value: unknown }) {
  return (
    <pre className="whitespace-pre-wrap break-all font-mono text-[11px] leading-5 text-muted-foreground">
      {JSON.stringify(value ?? {}, null, 2)}
    </pre>
  );
}

function addCurrentNodeReferences(
  catalog: ReferenceCatalog,
  data: ActionNodeData,
  manifest: NodeManifest,
  currentRawOutputLabel: string,
  nodeId?: string,
): ReferenceCatalog {
  const nativeSchema = asReferenceSchema(manifest.outputSchema);
  const itemPath = "item.json";
  const itemSelector = valueSelector("item");
  const itemFields = referenceFields(nativeSchema, itemPath, itemSelector);
  const nodePath = `outputs.${data.key}`;
  const ports = manifest.outputPorts.map((port) => {
    const portNativeSchema = asReferenceSchema(
      manifest.outputPortSchemas?.[port.name] ?? manifest.outputSchema,
    );
    const schema = portNativeSchema;
    const jsonPath = `${nodePath}.${port.name}.current.json`;
    const currentSelector = nodeId
      ? valueSelector("outputs", nodeId, port.name)
      : undefined;
    const fields = referenceFields(schema, jsonPath, currentSelector);
    const current: ReferenceEntry = {
      id: jsonPath,
      label: "current",
      path: jsonPath,
      selector: fields.length ? undefined : currentSelector,
      type: "object",
      schema,
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
      { id: nodePath, label: data.key, path: nodePath, sourceNodeType: data.nodeType, children: ports },
    ],
    item: [
      {
        id: itemPath,
        label: currentRawOutputLabel,
        path: itemPath,
        selector: itemFields.length ? undefined : itemSelector,
        type: "object",
        schema: nativeSchema,
        nullable: true,
        children: itemFields,
      },
    ],
  };
}

function addCurrentItemReference(
  catalog: ReferenceCatalog,
  label: string,
): ReferenceCatalog {
  return {
    ...catalog,
    item: [{
      id: "item.json",
      label,
      path: "item.json",
      selector: valueSelector("item"),
      type: "object",
      children: [],
    }],
  };
}

type ReferenceSchema = JsonSchemaProperty;

function asReferenceSchema(value: unknown): ReferenceSchema {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as ReferenceSchema)
    : {};
}

function referenceFields(
  schema: ReferenceSchema,
  parent: string,
  parentSelector?: ValueSelector,
): ReferenceEntry[] {
  return Object.entries(schema.properties ?? {}).map(([name, child]) => {
    const path = `${parent}.${name}`;
    const selector = parentSelector
      ? { ...parentSelector, path: [...parentSelector.path, name] }
      : undefined;
    const children = referenceFields(child, path, selector);
    return {
      id: path,
      label: name,
      path,
      selector: children.length ? undefined : selector,
      type: Array.isArray(child.type) ? child.type.join(" | ") : child.type,
      schema: child,
      nullable: !(schema.required ?? []).includes(name),
      children,
    };
  });
}

function valueSelector(
  namespace: ValueSelector["namespace"],
  sourceNodeId?: string,
  port?: string,
): ValueSelector {
  return {
    namespace,
    sourceNodeId,
    port,
    run: { kind: "current" },
    item: { kind: "current" },
    path: [],
  };
}

function parameterEntries(manifest?: NodeManifest) {
  return Object.entries(manifest?.parameterSchema.properties ?? {});
}
function uiFields(manifest?: NodeManifest): Record<string, UiField> {
  return manifest?.uiSchema.fields ?? {};
}
