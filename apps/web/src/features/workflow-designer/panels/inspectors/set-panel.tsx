import { useTranslation } from "react-i18next";
import { Plus, Trash2 } from "lucide-react";

import { Button } from "../../../../shared/ui/button";
import { Input } from "../../../../shared/ui/input";
import { asInputBinding, SmartInput } from "../../forms/binding-inputs";

import {
  CommonPanelSections,
  PanelSection,
  ParameterControl,
  type ActionPanelProps,
} from "./panel-shell";

export function SetPanel(panel: ActionPanelProps) {
  const { t } = useTranslation();
  return (
    <div className="space-y-5" data-testid="set-panel">
      <PanelSection
        description={t("studio.panels.set.valuesDescription")}
        title={t("studio.panels.set.values")}
        testId="set-values-section"
      >
        <SetRows panel={panel} />
      </PanelSection>
      <PanelSection title={t("studio.panels.set.options")} testId="set-options-section">
        <ParameterControl name="keepOnlySet" panel={panel} />
      </PanelSection>
      <CommonPanelSections panel={panel} />
    </div>
  );
}

function SetRows({ panel }: { panel: ActionPanelProps }) {
  const { t } = useTranslation();
  const binding = asInputBinding(panel.data.parameters.values ?? { kind: "object", fields: {} });
  const values = binding.kind === "object" ? binding.fields : {};
  const rows = Object.entries(values);
  const update = (next: Array<[string, unknown]>) => panel.onChange({ parameters: { ...panel.data.parameters, values: { kind: "object", fields: Object.fromEntries(next.filter(([name]) => name.trim())) } } });
  return <div className="space-y-2" data-testid="set-value-rows">
    {rows.map(([name, value], index) => <div className="grid grid-cols-[120px_minmax(0,1fr)_32px] gap-2" key={index}>
      <Input aria-label={t("studio.inspector.name")} onChange={(event) => update(rows.map((row, current) => current === index ? [event.target.value, row[1]] : row))} value={name} />
      <SmartInput allowedNamespaces={["inputs", "outputs", "contexts", "execution", "item", "loop"]} catalog={panel.referenceCatalog} onChange={(next) => update(rows.map((row, current) => current === index ? [row[0], next] : row))} value={asInputBinding(value)} />
      <Button aria-label={t("common.delete")} onClick={() => update(rows.filter((_, current) => current !== index))} size="icon" variant="ghost"><Trash2 className="size-3.5" /></Button>
    </div>)}
    <Button onClick={() => update([...rows, [`field_${rows.length + 1}`, { kind: "literal", value: "" }]])} size="sm" variant="ghost"><Plus className="size-3.5" />{t("common.create")}</Button>
  </div>;
}
