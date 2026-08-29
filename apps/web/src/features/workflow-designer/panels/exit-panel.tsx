import { LogOut, Pencil, Plus, Trash2, X } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "../../../shared/ui/button";
import { Dialog, DialogContent } from "../../../shared/ui/dialog";
import { Input } from "../../../shared/ui/input";
import { Select } from "../../../shared/ui/select";
import { Textarea } from "../../../shared/ui/textarea";
import { DynamicValueControl } from "../forms/parameter-field";
import { RequiredLabel } from "../forms/required-label";
import type {
  DynamicValue,
  ExitNodeData,
  JsonSchemaProperty,
  ReferenceCatalog,
  WorkflowEnd,
  WorkflowOutput,
} from "../model/types";
import {
  ArtifactConstraints,
  INPUT_TYPES,
  internalNameError,
  nextKey,
  outputSchemaKind,
  outputType,
  SchemaShapeEditor,
  SectionTitle,
  asObject,
  schemaForInputKind,
} from "./workflow-interface-panel";

type ValueUpdater<T> = T | ((current: T) => T);

const ERROR_FIELDS = [
  ["code", "string"], ["message", "string"], ["details", "object"],
  ["sourceNodeId", "string"], ["nodeExecutionId", "string"], ["retryable", "boolean"],
] as const;

export function ExitPanel({
  exitId,
  end,
  exits,
  onEndChange,
  onExitUpdate,
  onClose,
  referenceCatalog,
}: {
  exitId: string;
  end: WorkflowEnd;
  exits: Array<{ id: string; data: ExitNodeData }>;
  onEndChange: (value: ValueUpdater<WorkflowEnd>) => void;
  onExitUpdate: (id: string, patch: Partial<ExitNodeData>) => void;
  onClose: () => void;
  referenceCatalog?: ReferenceCatalog;
}) {
  const { t } = useTranslation();
  const current = exits.find((exit) => exit.id === exitId);
  const error = end.error ?? { strategy: "fail_fast" as const, collectWindowMs: 5000, outputs: {} };
  const errorCatalog = referenceCatalog ? { ...referenceCatalog, item: [{ id: "item.json", label: t("studio.references.currentError"), path: "item.json", selector: { namespace: "item" as const, run: { kind: "current" as const }, item: { kind: "current" as const }, path: [] }, type: "object", children: ERROR_FIELDS.map(([name, type]) => ({ id: `item.json.${name}`, label: t(`studio.errorFields.${name}`, name), path: `item.json.${name}`, selector: { namespace: "item" as const, run: { kind: "current" as const }, item: { kind: "current" as const }, path: [name] }, type, children: [] })) }] } : undefined;
  const renameExitMapping = (group: "outputs" | "errorOutputs") => (from: string, to: string) => {
    for (const exit of exits) {
      const mappings = exit.data.parameters[group];
      if (!(from in mappings)) continue;
      const next = { ...mappings };
      next[to] = next[from];
      delete next[from];
      onExitUpdate(exit.id, { parameters: { ...exit.data.parameters, [group]: next } });
    }
  };
  const removeExitMapping = (group: "outputs" | "errorOutputs") => (name: string) => {
    for (const exit of exits) {
      const mappings = exit.data.parameters[group];
      if (!(name in mappings)) continue;
      const next = { ...mappings };
      delete next[name];
      onExitUpdate(exit.id, { parameters: { ...exit.data.parameters, [group]: next } });
    }
  };
  return (
    <aside className="flex h-full w-[480px] shrink-0 flex-col overflow-hidden border-l border-border bg-surface" data-testid="exit-panel">
      <header className="flex h-16 items-center gap-3 border-b border-border px-4">
        <span className="grid size-9 place-items-center rounded-md bg-primary/10 text-primary"><LogOut className="size-5" /></span>
        <div className="min-w-0 flex-1"><strong className="block text-sm">{current?.data.label ?? t("studio.exit.title")}</strong><span className="text-[10px] text-muted-foreground">{t("studio.exit.description")}</span></div>
        <Button aria-label={t("common.close")} onClick={onClose} size="icon" variant="ghost"><X className="size-4" /></Button>
      </header>
      <div className="min-h-0 flex-1 space-y-6 overflow-y-auto p-4">
        <ContractSection
          description={t("studio.exit.contractDescription")}
          outputs={end.outputs}
          title={t("studio.interface.successOutputs")}
          onRenameMapping={renameExitMapping("outputs")}
          onRemoveMapping={removeExitMapping("outputs")}
          onSave={(update) => onEndChange((currentEnd) => ({ ...currentEnd, outputs: update(currentEnd.outputs) }))}
        />
        <section>
          <SectionTitle description={t("studio.interface.errorOutputsDescription")} title={t("studio.interface.errorOutputs")} />
          <div className="grid gap-2">
            <label className="text-xs"><span className="mb-1 block text-muted-foreground"><RequiredLabel required>{t("studio.interface.errorStrategy")}</RequiredLabel></span><Select aria-label={t("studio.interface.errorStrategy")} onValueChange={(strategy) => onEndChange((currentEnd) => ({ ...currentEnd, error: { ...normalizedError(currentEnd.error), strategy: strategy as WorkflowEnd["error"]["strategy"] } }))} options={[{ value: "fail_fast", label: t("studio.errorStrategies.fail_fast") }, { value: "collect", label: t("studio.errorStrategies.collect") }]} value={error.strategy} /></label>
            {error.strategy === "collect" && <label className="text-xs"><span className="mb-1 block text-muted-foreground"><RequiredLabel required>{t("studio.interface.collectWindow")}</RequiredLabel></span><Input aria-label={t("studio.interface.collectWindow")} min={100} max={60000} onChange={(event) => { const collectWindowMs = Math.min(60000, Math.max(100, Number(event.target.value) || 5000)); onEndChange((currentEnd) => ({ ...currentEnd, error: { ...normalizedError(currentEnd.error), collectWindowMs } })); }} type="number" value={String(error.collectWindowMs)} /></label>}
          </div>
          <div className="mt-5">
            <ContractSection
              description={t("studio.exit.errorContractDescription")}
              outputs={error.outputs}
              title={t("studio.interface.errorFields")}
              onRenameMapping={renameExitMapping("errorOutputs")}
              onRemoveMapping={removeExitMapping("errorOutputs")}
              onSave={(update) => onEndChange((currentEnd) => { const currentError = normalizedError(currentEnd.error); return { ...currentEnd, error: { ...currentError, outputs: update(currentError.outputs) } }; })}
            />
          </div>
        </section>
        {current && (
          <>
            <MappingSection
              catalog={referenceCatalog}
              contract={end.outputs}
              description={t("studio.exit.mappingDescription")}
              mappings={current.data.parameters.outputs}
              title={t("studio.exit.successMappings")}
              onChange={(name, value) => onExitUpdate(exitId, { parameters: { ...current.data.parameters, outputs: { ...current.data.parameters.outputs, [name]: value } } })}
            />
            <MappingSection
              catalog={errorCatalog}
              contract={error.outputs}
              description={t("studio.exit.errorMappingDescription")}
              mappings={current.data.parameters.errorOutputs}
              title={t("studio.exit.errorMappings")}
              onChange={(name, value) => onExitUpdate(exitId, { parameters: { ...current.data.parameters, errorOutputs: { ...current.data.parameters.errorOutputs, [name]: value } } })}
            />
          </>
        )}
      </div>
    </aside>
  );
}

function ContractSection({ title, description, outputs, onSave, onRenameMapping, onRemoveMapping }: { title: string; description?: string; outputs: Record<string, WorkflowOutput>; onSave: (update: (current: Record<string, WorkflowOutput>) => Record<string, WorkflowOutput>) => void; onRenameMapping: (from: string, to: string) => void; onRemoveMapping: (name: string) => void }) {
  const { t } = useTranslation();
  const [dialogOpen, setDialogOpen] = useState(false);
  const [editing, setEditing] = useState<{ name: string; originalName: string; original: WorkflowOutput; isNew: boolean } | null>(null);
  const add = () => {
    const name = nextKey(outputs, "output");
    setEditing({ name, originalName: name, original: { schema: { type: "string" }, required: false, sensitive: false }, isNew: true });
    setDialogOpen(true);
  };
  const edit = (name: string, output: WorkflowOutput) => {
    setEditing({ name, originalName: name, original: structuredClone(output), isNew: false });
    setDialogOpen(true);
  };
  const remove = (name: string) => { onSave((current) => { const next = { ...current }; delete next[name]; return next; }); onRemoveMapping(name); };
  const save = (name: string, output: WorkflowOutput) => {
    if (!editing) return;
    onSave((current) => {
      const next = { ...current };
      if (!editing.isNew && editing.originalName !== name) delete next[editing.originalName];
      next[name] = output;
      return next;
    });
    if (!editing.isNew && editing.originalName !== name) onRenameMapping(editing.originalName, name);
    setEditing(null);
    setDialogOpen(false);
  };
  return <section>
    <SectionTitle action={<Button onClick={add} size="sm" variant="secondary"><Plus className="size-3.5" />{t("studio.addField")}</Button>} description={description} title={title} />
    <div className="space-y-2">{Object.entries(outputs).map(([name, output]) => <ContractSummary key={name} name={name} output={output} onDelete={() => remove(name)} onEdit={() => edit(name, output)} />)}</div>
    {editing && <ContractFieldDialog editing={{ name: editing.name, originalName: editing.originalName, output: editing.original, isNew: editing.isNew }} existing={outputs} onOpenChange={(open) => { if (!open) { setEditing(null); setDialogOpen(false); } }} onSave={save} open={dialogOpen} />}
  </section>;
}

function ContractSummary({ name, output, onEdit, onDelete }: { name: string; output: WorkflowOutput; onEdit: () => void; onDelete: () => void }) {
  const { t } = useTranslation();
  const schema = asObject(output.schema);
  return <div className="flex items-center gap-3 rounded-md border border-border px-3 py-2.5" data-testid="exit-contract-field"><div className="min-w-0 flex-1"><div className="flex items-center gap-2"><span className="truncate text-xs font-medium">{String(schema.title ?? name)}</span><span className="rounded bg-muted px-1.5 py-0.5 text-[10px] text-muted-foreground">{t(`studio.schemaTypes.${outputSchemaKind(output.schema)}`, outputSchemaKind(output.schema))}</span>{output.required && <span className="text-[10px] text-danger">{t("studio.interface.required")}</span>}{output.sensitive && <span className="text-[10px] text-warning">{t("studio.interface.sensitive")}</span>}</div><div className="truncate font-mono text-[10px] text-muted-foreground">{name}</div></div><Button aria-label={t("studio.editField")} onClick={onEdit} size="icon" variant="ghost"><Pencil className="size-3.5" /></Button><Button aria-label={t("studio.removeField")} onClick={onDelete} size="icon" variant="ghost"><Trash2 className="size-3.5" /></Button></div>;
}

function ContractFieldDialog({ open, onOpenChange, editing, existing, onSave }: { open: boolean; onOpenChange: (open: boolean) => void; editing: { name: string; originalName: string; output: WorkflowOutput; isNew: boolean }; existing: Record<string, WorkflowOutput>; onSave: (name: string, output: WorkflowOutput) => void }) {
  const { t } = useTranslation();
  const [name, setName] = useState("");
  const [output, setOutput] = useState<WorkflowOutput>({ schema: { type: "string" }, required: false, sensitive: false });
  useEffect(() => {
    if (open) { setName(editing.name); setOutput(editing.output); }
  }, [open, editing]);
  const update = (patch: Partial<WorkflowOutput>) => setOutput((current) => ({ ...current, ...patch }));
  const updateType = (type: string) => update({ schema: schemaForInputKind(type, asObject(output.schema) as never) });
  const schema = asObject(output.schema);
  const nameError = internalNameError(name, existing, editing.originalName, t);
  return <Dialog onOpenChange={onOpenChange} open={open}><DialogContent description={t("studio.exit.contractDialogDescription")} title={t("studio.interface.outputField")}><div className="space-y-4 p-5"><div className="grid gap-3"><label className="text-xs"><span className="mb-1 block text-muted-foreground"><RequiredLabel required>{t("studio.interface.fieldType")}</RequiredLabel></span><Select aria-label={t("studio.interface.fieldType")} onValueChange={updateType} options={INPUT_TYPES.map((value) => ({ value, label: t(`studio.schemaTypes.${value}`, value) }))} value={outputSchemaKind(output.schema)} /></label><label className="text-xs"><span className="mb-1 block text-muted-foreground"><RequiredLabel required>{t("studio.interface.outputName")}</RequiredLabel></span><Input aria-invalid={Boolean(nameError)} aria-label={t("studio.interface.outputName")} onChange={(event) => setName(event.target.value)} value={name} />{nameError && <span className="mt-1 block text-[10px] text-danger" role="alert">{nameError}</span>}</label><label className="text-xs"><span className="mb-1 block text-muted-foreground">{t("studio.interface.fieldTitle")}</span><Input onChange={(event) => update({ schema: { ...schema, title: event.target.value } })} value={String(schema.title ?? "")} /></label>{!asObject(output.schema)["x-agentx-artifact"] && (outputType(output.schema) === "object" || outputType(output.schema) === "array") && <SchemaShapeEditor onChange={(next) => update({ schema: next })} schema={output.schema as JsonSchemaProperty} />}{asObject(output.schema)["x-agentx-artifact"] && <ArtifactConstraints onChange={(patch) => update({ schema: { ...schema, ...patch } })} property={schema as never} t={(key, fallback) => (fallback ? t(key, { defaultValue: fallback }) : t(key))} />}<div className="grid grid-cols-2 gap-3 text-xs"><label className="flex items-center gap-2"><input checked={output.required} className="size-4 accent-primary" onChange={(event) => update({ required: event.target.checked })} type="checkbox" />{t("studio.interface.required")}</label><label className="flex items-center gap-2"><input checked={output.sensitive} className="size-4 accent-primary" onChange={(event) => update({ sensitive: event.target.checked })} type="checkbox" />{t("studio.interface.sensitive")}</label></div><label className="text-xs"><span className="mb-1 block text-muted-foreground">{t("studio.interface.description")}</span><Textarea className="min-h-14" onChange={(event) => update({ schema: { ...schema, description: event.target.value } })} value={String(schema.description ?? "")} /></label></div><p className="text-[10px] text-muted-foreground">{t("studio.exit.contractRenameHint")}</p><div className="flex justify-end gap-2 border-t border-border pt-4"><Button onClick={() => onOpenChange(false)} variant="ghost">{t("common.cancel")}</Button><Button disabled={Boolean(nameError)} onClick={() => onSave(name.trim(), output)}>{t("common.save")}</Button></div></div></DialogContent></Dialog>;
}

function MappingSection({ title, description, contract, mappings, onChange, catalog }: { title: string; description?: string; contract: Record<string, WorkflowOutput>; mappings: Record<string, DynamicValue>; onChange: (name: string, value: DynamicValue) => void; catalog?: ReferenceCatalog }) {
  const { t } = useTranslation();
  const names = Object.keys(contract);
  return <section>
    <SectionTitle description={description} title={title} />
    {names.length === 0 && <p className="rounded-md border border-dashed border-border px-3 py-4 text-center text-[10px] text-muted-foreground">{t("studio.exit.emptyContract")}</p>}
    <div className="space-y-3">{names.map((name) => {
      const output = contract[name];
      return <div key={name} className="rounded-md border border-border px-3 py-2.5" data-testid={`exit-mapping-${name}`}>
        <div className="mb-2 flex items-center gap-2"><span className="truncate text-xs font-medium">{String(asObject(output.schema).title ?? name)}</span><span className="rounded bg-muted px-1.5 py-0.5 text-[10px] text-muted-foreground">{t(`studio.schemaTypes.${outputSchemaKind(output.schema)}`, outputSchemaKind(output.schema))}</span>{output.required && <span className="text-[10px] text-danger">{t("studio.interface.required")}</span>}</div>
        <DynamicValueControl allowed={["inputs", "outputs", "contexts", "execution"]} catalog={catalog} expectedType={outputType(output.schema)} onChange={(value) => onChange(name, value)} value={mappings[name] ?? { kind: "literal", value: "" }} />
      </div>;
    })}</div>
  </section>;
}

function normalizedError(error: WorkflowEnd["error"] | undefined): NonNullable<WorkflowEnd["error"]> { return error ?? { strategy: "fail_fast", collectWindowMs: 5000, outputs: {} }; }
