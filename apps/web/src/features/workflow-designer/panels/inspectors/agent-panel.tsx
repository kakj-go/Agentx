import { ChevronDown, ChevronUp, Settings2, X } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "../../../../shared/ui/button";
import { Select } from "../../../../shared/ui/select";
import {
  CommonPanelSections,
  PanelField,
  PanelSection,
  ParameterControl,
  ResourceSelect,
  resourceOptionsFor,
  resourceSelectors,
  type ActionPanelProps,
} from "./panel-shell";

const CORE_PARAMETERS = ["systemPrompt", "userQuestion", "sessionPolicy"];

export function AgentPanel(panel: ActionPanelProps) {
  const { t } = useTranslation();
  const [advancedOpen, setAdvancedOpen] = useState(false);
  const entries = Object.keys(panel.manifest.parameterSchema.properties ?? {});
  const advancedFields = entries.filter((name) => !CORE_PARAMETERS.includes(name));
  const summary = [
    panel.data.parameters.maxIterations ? t("studio.panels.agent.iterationSummary", { count: panel.data.parameters.maxIterations }) : undefined,
    panel.data.parameters.maxToolCalls !== undefined ? t("studio.panels.agent.toolSummary", { count: panel.data.parameters.maxToolCalls }) : undefined,
    panel.data.parameters.maxDurationMs ? t("studio.panels.agent.durationSummary", { count: Math.round(Number(panel.data.parameters.maxDurationMs) / 1000) }) : undefined,
  ].filter(Boolean).join(" · ");
  return (
    <div className="space-y-5" data-testid="agent-panel">
      <AgentCoreConfiguration panel={panel} />
      <PanelSection title={t("studio.panels.agent.task")} testId="agent-task-section">
        <div className="space-y-4">
          <ParameterControl name="systemPrompt" panel={panel} />
          <ParameterControl name="userQuestion" panel={panel} />
        </div>
      </PanelSection>
      <PanelSection
        description={t("studio.panels.agent.budgetHint")}
        title={t("studio.panels.agent.budget")}
        testId="agent-budget-section"
      >
        <section className="rounded-md border border-border/70">
          <div className="flex items-center gap-2 px-3 py-3">
            <Settings2 className="size-3.5 text-primary" />
            <div className="min-w-0 flex-1">
              <div className="text-xs font-semibold">
                {summary || t("studio.panels.agent.budgetCollapsed")}
              </div>
              <div className="mt-0.5 text-[10px] text-muted-foreground">
                {t("studio.panels.agent.budgetHint")}
              </div>
            </div>
            <Button
              aria-label={t("studio.inspector.toggleAdvanced")}
              onClick={() => setAdvancedOpen((value) => !value)}
              size="icon"
              variant="ghost"
            >
              {advancedOpen ? <ChevronUp className="size-3.5" /> : <ChevronDown className="size-3.5" />}
            </Button>
          </div>
          {advancedOpen && advancedFields.length > 0 && (
            <div className="grid grid-cols-2 gap-3 border-t border-border p-3">
              {advancedFields.map((name) => (
                <ParameterControl key={name} name={name} panel={panel} />
              ))}
            </div>
          )}
        </section>
      </PanelSection>
      <CommonPanelSections panel={panel} />
    </div>
  );
}

function AgentCoreConfiguration({ panel }: { panel: ActionPanelProps }) {
  const { t } = useTranslation();
  const sessionPolicy = panel.data.parameters.sessionPolicy as { mode?: string } | undefined;
  const selectors = resourceSelectors(panel.manifest);
  const inspectorSlots = panel.manifest.bindingSlots.filter(
    (slot) => slot.placement === "inspector",
  );
  return (
    <section className="space-y-4 rounded-md border border-border/70 p-3" data-testid="agent-core-configuration">
      <div>
        <div className="text-xs font-semibold">{t("studio.inspector.agentCore")}</div>
        <div className="mt-0.5 text-[10px] text-muted-foreground">
          {t("studio.inspector.agentCoreDescription")}
        </div>
      </div>
      {inspectorSlots.map((slot) => {
        const selector = selectors.find((candidate) => candidate.bindingRole === slot.name);
        const operation = selector?.operation ?? "use";
        const references = panel.data.resourceReferences.filter(
          (candidate) => candidate.bindingRole === slot.name,
        );
        const reference = references[0];
        const options = resourceOptionsFor(panel.resources, slot.resourceType, operation);
        return (
          <div key={slot.name}>
            {slot.multiple && references.length > 0 && (
              <div className="mb-2 flex flex-wrap gap-1.5">
                {references.map((selected) => (
                  <span className="inline-flex items-center gap-1 rounded-md border border-border bg-muted/40 px-2 py-1 text-[10px]" key={`${slot.name}:${selected.resourceId}`}>
                    {options.find((option) => option.value === selected.resourceId)?.label ?? selected.resourceId}
                    <button aria-label={t("studio.inspector.clearResource")} onClick={() => panel.onChange({ resourceReferences: panel.data.resourceReferences.filter((candidate) => !(candidate.bindingRole === slot.name && candidate.resourceId === selected.resourceId)) })} type="button"><X className="size-3" /></button>
                  </span>
                ))}
              </div>
            )}
            <ResourceSelect
              error={panel.fieldErrors[`resourceReferences.${slot.name}`]}
              fieldPath={`resourceReferences.${slot.name}`}
              label={
                panel.localized?.bindingSlotLabel(slot.name) ??
                selector?.label ??
                slot.name.replaceAll("_", " ")
              }
              onAuthorize={panel.onResourceAuthorize}
              onChange={(resourceId, versionId) => {
                const kept = panel.data.resourceReferences.filter((candidate) => slot.multiple || candidate.bindingRole !== slot.name)
                if (slot.multiple && references.some((candidate) => candidate.resourceId === resourceId)) return
                panel.onChange({ resourceReferences: [...kept, { bindingRole: slot.name, resourceType: slot.resourceType, resourceId, resourceVersionId: versionId, operation }] })
              }}
              onClear={
                slot.required
                  ? undefined
                  : () =>
                      panel.onChange({
                        resourceReferences: panel.data.resourceReferences.filter(
                            (candidate) => candidate.bindingRole !== slot.name,
                        ),
                      })
              }
              onRequest={panel.onResourceRequest}
              options={options}
              optionsMissing={
                slot.required &&
                !options.length
              }
              required={slot.required}
              sourceNodeId={panel.sourceNodeId}
              testId={`agent-inspector-${slot.name}`}
              value={slot.multiple ? undefined : reference?.resourceId}
            />
            {slot.name === "workspace_sandbox" && !reference && (
              <p className="mt-1.5 text-[10px] leading-4 text-warning">
                {t("studio.inspector.sandboxToolsDisabled")}
              </p>
            )}
          </div>
        );
      })}
      <PanelField
        error={panel.fieldErrors["parameters.sessionPolicy"]}
        fieldPath="parameters.sessionPolicy"
        label={t("studio.inspector.sessionPolicy")}
        required
      >
        <Select
          className="w-full"
          onValueChange={(mode) =>
            panel.onChange({ parameters: { ...panel.data.parameters, sessionPolicy: { mode } } })
          }
          options={[
            { value: "application_session", label: t("studio.inspector.sessionApplication") },
            { value: "invocation", label: t("studio.inspector.sessionInvocation") },
          ]}
          placeholder={t("studio.inspector.selectSessionPolicy")}
          value={sessionPolicy?.mode ?? ""}
        />
      </PanelField>
    </section>
  );
}
