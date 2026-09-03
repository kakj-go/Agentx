import { useTranslation } from "react-i18next";
import { Plus, Trash2 } from "lucide-react";

import { Button } from "../../../../shared/ui/button";
import { Select } from "../../../../shared/ui/select";
import { asReferenceBinding, ReferenceInput } from "../../forms/binding-inputs";
import {
  CommonPanelSections,
  PanelHint,
  PanelSection,
  ParameterControl,
  type ActionPanelProps,
} from "./panel-shell";

export function ListPanel(panel: ActionPanelProps) {
  const { t } = useTranslation();
  return (
    <div className="space-y-5" data-testid="list-panel">
      <PanelSection title={t("studio.panels.list.input")} testId="list-input-section">
        <ParameterControl name="input" panel={panel} />
        <PanelHint>{t("studio.panels.list.inputHint")}</PanelHint>
      </PanelSection>
      <PanelSection title={t("studio.panels.list.filter")} testId="list-filter-section">
        <ParameterControl name="filter" panel={panel} />
      </PanelSection>
      <PanelSection title={t("studio.panels.list.sort")} testId="list-sort-section">
        <SortRows panel={panel} />
      </PanelSection>
      <PanelSection title={t("studio.panels.list.limit")} testId="list-limit-section">
        <ParameterControl name="takeN" panel={panel} />
      </PanelSection>
      <CommonPanelSections panel={panel} />
    </div>
  );
}

function SortRows({ panel }: { panel: ActionPanelProps }) {
  const { t } = useTranslation();
  const rows = Array.isArray(panel.data.parameters.sort) ? panel.data.parameters.sort as Array<{ selector?: unknown; direction?: string; nulls?: string }> : [];
  const update = (next: typeof rows) => panel.onChange({ parameters: { ...panel.data.parameters, sort: next } });
  return <div className="space-y-2" data-testid="list-sort-rows">
    {rows.map((row, index) => <div className="rounded-md border border-border/70 p-2" key={index}>
      <div className="grid grid-cols-[minmax(0,1fr)_112px_32px] gap-2">
        <ReferenceInput allowedNamespaces={["item"]} catalog={panel.referenceCatalog} onChange={(selector) => update(rows.map((item, current) => current === index ? { ...item, selector } : item))} value={asReferenceBinding(row.selector)} />
        <Select onValueChange={(direction) => update(rows.map((item, current) => current === index ? { ...item, direction } : item))} options={[{ value: "asc", label: t("studio.panels.list.ascending") }, { value: "desc", label: t("studio.panels.list.descending") }]} value={row.direction ?? "asc"} />
        <Button aria-label={t("common.delete")} onClick={() => update(rows.filter((_, current) => current !== index))} size="icon" variant="ghost"><Trash2 className="size-3.5" /></Button>
      </div>
      <details className="mt-1 text-[10px] text-muted-foreground"><summary className="cursor-pointer">{t("studio.panels.list.sortAdvanced")}</summary><div className="mt-2 w-40"><Select onValueChange={(nulls) => update(rows.map((item, current) => current === index ? { ...item, nulls } : item))} options={[{ value: "first", label: t("studio.panels.list.nullsFirst") }, { value: "last", label: t("studio.panels.list.nullsLast") }]} value={row.nulls ?? "last"} /></div></details>
    </div>)}
    <Button onClick={() => update([...rows, { direction: "asc", nulls: "last" }])} size="sm" variant="ghost"><Plus className="size-3.5" />{t("common.create")}</Button>
  </div>;
}
