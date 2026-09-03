import { useTranslation } from "react-i18next";

import {
  CommonPanelSections,
  OutputContractHint,
  PanelHint,
  PanelSection,
  ParameterControl,
  type ActionPanelProps,
} from "./panel-shell";

export function SubworkflowPanel(panel: ActionPanelProps) {
  const { t } = useTranslation();
  const selectedManifest = panel.providerOptions.workflowVersionId?.find(
    (option) => option.value === panel.data.parameters.workflowVersionId,
  )?.manifest;
  const effectivePanel = selectedManifest ? { ...panel, manifest: selectedManifest } : panel;
  const targetPanel = {
    ...panel,
    onChange: (patch: Parameters<ActionPanelProps["onChange"]>[0]) => {
      const next = patch.parameters;
      if (next && next.workflowVersionId !== panel.data.parameters.workflowVersionId) {
        panel.onChange({ ...patch, parameters: { ...next, inputs: { kind: "object", fields: {} } } });
      } else {
        panel.onChange(patch);
      }
    },
  };
  return (
    <div className="space-y-5" data-testid="subworkflow-panel">
      <PanelSection
        description={t("studio.panels.subworkflow.targetDescription")}
        title={t("studio.panels.subworkflow.target")}
        testId="subworkflow-target-section"
      >
        <ParameterControl name="workflowVersionId" panel={targetPanel} />
      </PanelSection>
      <PanelSection title={t("studio.panels.subworkflow.inputs")} testId="subworkflow-inputs-section">
        <ParameterControl name="inputs" panel={effectivePanel} />
      </PanelSection>
      <PanelSection title={t("studio.panels.subworkflow.output")} testId="subworkflow-output-section">
        <PanelHint>{t("studio.panels.subworkflow.outputHint")}</PanelHint>
        <OutputContractHint manifest={effectivePanel.manifest} />
      </PanelSection>
      <CommonPanelSections panel={effectivePanel} />
    </div>
  );
}
