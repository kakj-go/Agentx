import { useTranslation } from "react-i18next";
import { Button } from "../../../../shared/ui/button";

import {
  CommonPanelSections,
  OutputContractHint,
  PanelHint,
  PanelSection,
  ParameterControl,
  ResourceSelectorFields,
  type ActionPanelProps,
} from "./panel-shell";

export function ModelPanel(panel: ActionPanelProps) {
  const { t } = useTranslation();
  const structured = panel.data.parameters.responseMode === "json_schema";
  const effectiveManifest = structured && panel.data.parameters.structuredSchema
    ? { ...panel.manifest, outputSchema: { ...(panel.manifest.outputSchema as Record<string, unknown>), properties: { ...((panel.manifest.outputSchema as { properties?: Record<string, unknown> }).properties ?? {}), structuredOutput: panel.data.parameters.structuredSchema } } }
    : panel.manifest;
  const effectivePanel = { ...panel, manifest: effectiveManifest };
  return (
    <div className="space-y-5" data-testid="model-panel">
      <PanelSection
        description={t("studio.panels.model.modelDescription")}
        title={t("studio.panels.model.model")}
        testId="model-model-section"
      >
        <ResourceSelectorFields panel={panel} />
      </PanelSection>
      <PanelSection title={t("studio.panels.model.prompt")} testId="model-prompt-section">
        <div className="space-y-4">
          <ParameterControl name="prompt" panel={panel} />
          <ParameterControl name="userQuestion" panel={panel} />
        </div>
        <div className="mt-3">
          <PanelHint>{t("studio.panels.model.variableHint")}</PanelHint>
        </div>
      </PanelSection>
      <PanelSection title={t("studio.panels.model.output")} testId="model-output-section">
        <ParameterControl name="responseMode" panel={panel} />
        {structured && <div className="mt-4 space-y-2">
          {!panel.data.parameters.structuredSchema && <Button data-testid="model-schema-template" onClick={() => panel.onChange({ parameters: { structuredSchema: { type: "object", properties: { answer: { type: "string" } }, required: ["answer"], additionalProperties: false } } })} size="sm" variant="secondary">{t("studio.panels.model.initializeSchema")}</Button>}
          <ParameterControl name="structuredSchema" panel={panel} />
        </div>}
        <OutputContractHint manifest={effectiveManifest} />
      </PanelSection>
      <CommonPanelSections panel={effectivePanel} />
    </div>
  );
}
