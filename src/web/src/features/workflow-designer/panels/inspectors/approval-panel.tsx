import { useTranslation } from "react-i18next";
import { useEffect, useState } from "react";

import { Input } from "../../../../shared/ui/input";
import { Select } from "../../../../shared/ui/select";

import {
  CommonPanelSections,
  PanelHint,
  PanelSection,
  ParameterControl,
  type ActionPanelProps,
} from "./panel-shell";

export function ApprovalPanel(panel: ActionPanelProps) {
  const { t } = useTranslation();
  // Default to the frozen approve/reject pair so the panel shows the same
  // branches the card renders for an unconfigured node.
  const buttons = panel.data.parameters.buttons;
  useEffect(() => {
    if (!Array.isArray(buttons) || buttons.length === 0) {
      panel.onChange({
        parameters: {
          ...panel.data.parameters,
          buttons: [
            { id: "approved", label: t("studio.card.approveDefault") },
            { id: "rejected", label: t("studio.card.rejectDefault") },
          ],
        },
      });
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  return (
    <div className="space-y-5" data-testid="approval-panel">
      <PanelSection
        description={t("studio.panels.approval.contentDescription")}
        title={t("studio.panels.approval.content")}
        testId="approval-content-section"
      >
        <div className="space-y-4">
          <ParameterControl name="title" panel={panel} />
          <ParameterControl name="description" panel={panel} />
          <ParameterControl name="candidateUserId" panel={panel} />
        </div>
      </PanelSection>
      <PanelSection title={t("studio.panels.approval.branches")} testId="approval-branches-section">
        <ParameterControl name="buttons" panel={panel} />
        <div className="mt-3">
          <PanelHint>{t("studio.panels.approval.branchesHint")}</PanelHint>
        </div>
      </PanelSection>
      <PanelSection title={t("studio.panels.approval.timeout")} testId="approval-timeout-section">
        <ApprovalTimeout panel={panel} />
        <div className="mt-3">
          <PanelHint>{t("studio.panels.approval.timeoutHint")}</PanelHint>
        </div>
      </PanelSection>
      <CommonPanelSections panel={panel} />
    </div>
  );
}

function ApprovalTimeout({ panel }: { panel: ActionPanelProps }) {
  const { t } = useTranslation();
  const timeoutMs = typeof panel.data.parameters.timeoutMs === "number" ? panel.data.parameters.timeoutMs : undefined;
  const [unit, setUnit] = useState<"minutes" | "hours" | "days">("hours");
  const factors = { minutes: 60_000, hours: 3_600_000, days: 86_400_000 } as const;
  const setTimeoutMs = (value?: number) => {
    panel.onChange({ parameters: { timeoutMs: value } });
  };
  return <div className="space-y-3">
    <label className="flex items-center gap-2 text-xs"><input checked={timeoutMs !== undefined} className="size-4 accent-primary" data-testid="approval-timeout-enabled" onChange={(event) => setTimeoutMs(event.target.checked ? factors[unit] : undefined)} type="checkbox" />{t("studio.panels.approval.timeout")}</label>
    {timeoutMs !== undefined && <div className="grid grid-cols-[minmax(0,1fr)_110px] gap-2">
      <Input data-testid="parameter-timeoutMs" min={1} onChange={(event) => setTimeoutMs(Math.max(1, Number(event.target.value)) * factors[unit])} type="number" value={Math.max(1, Math.round(timeoutMs / factors[unit]))} />
      <Select onValueChange={(value) => setUnit(value as typeof unit)} options={[{ value: "minutes", label: t("studio.units.minutes") }, { value: "hours", label: t("studio.units.hours") }, { value: "days", label: t("studio.units.days") }]} value={unit} />
    </div>}
  </div>;
}
