import { useTranslation } from "react-i18next";
import { useEffect } from "react";

import {
  CommonPanelSections,
  PanelHint,
  PanelSection,
  ParameterControl,
  type ActionPanelProps,
} from "./panel-shell";

const defaultCondition = () => ({
  condition: {
    left: { kind: "literal", value: "" },
    operator: "eq",
    right: { kind: "literal", value: "" },
  },
});
const DEFAULT_CASES = [{ id: "case_1", name: "", conditions: [defaultCondition()], logicalOp: "and" }];

export function IfPanel(panel: ActionPanelProps) {
  const { t } = useTranslation();
  // A fresh branch should be immediately editable without requiring a second
  // click on "Add condition".
  const cases = panel.data.parameters.cases;
  useEffect(() => {
    if (!Array.isArray(cases) || cases.length === 0) {
      panel.onChange({ parameters: { ...panel.data.parameters, cases: DEFAULT_CASES } });
      return;
    }
    const normalized = cases.map((branch) => {
      if (!branch || typeof branch !== "object") return branch;
      const conditions = (branch as { conditions?: unknown }).conditions;
      return Array.isArray(conditions) && conditions.length > 0
        ? branch
        : { ...branch, conditions: [defaultCondition()] };
    });
    if (normalized.some((branch, index) => branch !== cases[index])) {
      panel.onChange({ parameters: { ...panel.data.parameters, cases: normalized } });
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  return (
    <div className="space-y-5" data-testid="if-panel">
      <PanelSection title={t("studio.panels.if.branches")} testId="if-branches-section">
        <ParameterControl name="cases" panel={panel} />
        <div
          className="mt-2 rounded-md border border-border/70 bg-muted/20 p-2"
          data-testid="if-else-branch"
        >
          <div className="flex items-center gap-2">
            <span className="shrink-0 rounded bg-muted px-1.5 py-0.5 text-[10px] font-medium text-muted-foreground">
              ELSE
            </span>
            <span className="text-[10px] text-muted-foreground">
              {t("studio.panels.if.elseHint")}
            </span>
          </div>
        </div>
      </PanelSection>
      <PanelSection title={t("studio.panels.if.notes")} testId="if-notes-section">
        <PanelHint>{t("studio.panels.if.notesHint")}</PanelHint>
      </PanelSection>
      <CommonPanelSections panel={panel} />
    </div>
  );
}
