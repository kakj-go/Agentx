import { useTranslation } from "react-i18next";

import {
  CommonPanelSections,
  ModeCardsGroup,
  PanelHint,
  PanelSection,
  ParameterControl,
  type ActionPanelProps,
} from "./panel-shell";

const ERROR_MODE_PARAMETER = "errorMode";

export function LoopPanel(panel: ActionPanelProps) {
  const { t } = useTranslation();
  const errorModeSchema = panel.manifest.parameterSchema.properties?.[ERROR_MODE_PARAMETER];
  const errorMode = String(
    panel.data.parameters[ERROR_MODE_PARAMETER] ?? errorModeSchema?.default ?? "",
  );
  return (
    <div className="space-y-5" data-testid="loop-panel">
      <PanelSection title={t("studio.panels.loop.stepInput")} testId="loop-input-section">
        <ParameterControl name="input" panel={panel} />
        <PanelHint>{t("studio.panels.loop.inputHint")}</PanelHint>
      </PanelSection>
      <PanelSection title={t("studio.panels.loop.stepBody")} testId="loop-body-section">
        <PanelHint>{t("studio.panels.loop.bodyHint")}</PanelHint>
      </PanelSection>
      <PanelSection title={t("studio.panels.loop.stepOutput")} testId="loop-output-section">
        <ParameterControl name="outputSelector" panel={panel} />
        <PanelHint>{t("studio.panels.loop.outputHint")}</PanelHint>
      </PanelSection>
      <PanelSection title={t("studio.panels.loop.execution")} testId="loop-execution-section">
        <ModeCardsGroup
          onChange={(value) =>
            panel.onChange({
              parameters: { ...panel.data.parameters, [ERROR_MODE_PARAMETER]: value },
            })
          }
          options={(errorModeSchema?.enum ?? []).map(String).map((value) => ({
            value,
            label: t(`studio.panels.loop.errorModes.${value}.label`),
            description: t(`studio.panels.loop.errorModes.${value}.description`),
          }))}
          value={errorMode}
        />
        <div className="mt-4">
          <ParameterControl name="parallelism" panel={panel} />
        </div>
      </PanelSection>
      <PanelSection title={t("studio.panels.loop.builtins")} testId="loop-builtins-section">
        <PanelHint>
          <code className="font-mono text-foreground">loop.item</code>
          {" · "}
          <code className="font-mono text-foreground">loop.items</code>
          {" · "}
          <code className="font-mono text-foreground">loop.index</code>
          <span className="mt-1 block">{t("studio.panels.loop.builtinsHint")}</span>
        </PanelHint>
      </PanelSection>
      <CommonPanelSections panel={panel} />
    </div>
  );
}
