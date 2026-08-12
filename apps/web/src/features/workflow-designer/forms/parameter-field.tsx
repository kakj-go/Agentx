import { AlertTriangle, Plus, Trash2 } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "../../../shared/ui/button";
import { Input } from "../../../shared/ui/input";
import { Select } from "../../../shared/ui/select";
import { Textarea } from "../../../shared/ui/textarea";
import type {
  JsonSchemaProperty,
  ReferenceCatalog,
  ReferenceNamespace,
  ResourceOption,
  UiField,
} from "../model/types";
import { CodeEditor, type CodeEditorHandle } from "./code-editor";
import { ExpressionEditor } from "./expression-editor";
import { ReferencePicker } from "./reference-picker/reference-picker";
import { insertAtSelection } from "./reference-picker/reference-insertion";

const EMPTY_JSON_OBJECT = {};
const EMPTY_JSON_ARRAY: unknown[] = [];

export function ParameterField({
  name,
  schema,
  ui,
  value,
  required,
  error,
  parameters,
  providerOptions = [],
  workflowId,
  referenceCatalog,
  onChange,
  onValidityChange,
  labelOverride,
  descriptionOverride,
  enumLabels,
  nestedLocalization,
}: {
  name: string;
  schema: JsonSchemaProperty;
  ui?: UiField;
  value: unknown;
  required?: boolean;
  error?: string;
  parameters: Record<string, unknown>;
  providerOptions?: ResourceOption[];
  workflowId?: string;
  referenceCatalog?: ReferenceCatalog;
  onChange: (value: unknown) => void;
  onValidityChange?: (valid: boolean) => void;
  labelOverride?: string;
  descriptionOverride?: string;
  enumLabels?: Record<string, string>;
  nestedLocalization?: StructuredFieldLocalization;
}) {
  const { t } = useTranslation();
  const label = labelOverride ?? ui?.label ?? schema.title ?? humanize(name);
  const visible =
    !ui?.visibleWhen ||
    parameters[ui.visibleWhen.field] === ui.visibleWhen.equals;
  const control = ui?.control;
  const textReferenceEnabled = Boolean(referenceCatalog) && schema.templatable === true;
  useEffect(() => {
    onValidityChange?.(control ? SUPPORTED_CONTROLS.has(control) : false);
  }, [control, onValidityChange]);
  if (!visible) return null;
  if (!control || !SUPPORTED_CONTROLS.has(control))
    return <UnsupportedControl label={label} control={control} />;
  const field = (content: React.ReactNode) => (
    <Field
      error={error}
      label={label}
      required={required}
      testId={`parameter-${name}`}
      unit={(() => { const value = ui?.unit ?? inferredUnit(name); return value ? t(`studio.units.${value}`, value) : undefined; })()}
    >
      {content}
      {descriptionOverride && <p className="mt-1 text-[10px] text-muted-foreground">{descriptionOverride}</p>}
    </Field>
  );
  if (control === "select")
    return field(
      <Select
        className="w-full"
        onValueChange={onChange}
        options={(ui.options ?? schema.enum ?? []).map((item) =>
          selectOption(item, enumLabels?.[String(item)]),
        )}
        value={value === undefined ? "" : String(value)}
      />,
    );
  if (control === "provider_options")
    return field(
      <Select
        className="w-full"
        onValueChange={onChange}
        options={providerOptions}
        value={value === undefined ? "" : String(value)}
      />,
    );
  if (control === "boolean")
    return (
      <div>
        <label className="flex items-center gap-2 text-xs">
          <input
            checked={Boolean(value)}
            className="size-4 accent-primary"
            onChange={(event) => onChange(event.target.checked)}
            type="checkbox"
          />
          <span>{label}</span>
        </label>
        {error && <p className="mt-1 text-[10px] text-danger">{error}</p>}
      </div>
    );
  if (control === "number")
    return field(
      <Input
        max={schema.maximum}
        min={schema.minimum}
        onChange={(event) =>
          onChange(
            event.target.value === "" ? undefined : Number(event.target.value),
          )
        }
        type="number"
        value={value === undefined ? "" : String(value)}
      />,
    );
  if (control === "textarea")
    return field(
      <NativeReferenceControl
        catalog={referenceCatalog}
        enabled={textReferenceEnabled}
        multiline
        onChange={onChange}
        schema={schema}
        value={String(value ?? "")}
      />,
    );
  if (control === "prompt")
    return field(
      <NativeReferenceControl
        catalog={referenceCatalog}
        enabled={textReferenceEnabled}
        multiline
        multilineClassName="min-h-40"
        onChange={onChange}
        schema={schema}
        value={String(value ?? "")}
      />,
    );
  if (control === "expression")
    return field(
      <ExpressionReferenceControl
        catalog={referenceCatalog}
        enabled={textReferenceEnabled}
        onChange={onChange}
        schema={schema}
        value={String(value ?? "")}
        workflowId={workflowId}
      />,
    );
  if (control === "json")
    return field(
      <StructuredJsonControl
        catalog={referenceCatalog}
        enabled={textReferenceEnabled}
        fallback={
          schema.type === "array" ? EMPTY_JSON_ARRAY : EMPTY_JSON_OBJECT
        }
        onChange={onChange}
        onValidityChange={onValidityChange}
        schema={schema}
        value={value}
        localization={nestedLocalization}
        path={name}
      />,
    );
  if (control === "code")
    return field(
      <CodeControl
        language={codeLanguage(
          ui.languageField ? parameters[ui.languageField] : undefined,
        )}
        onChange={onChange}
        value={String(value ?? "")}
      />,
    );
  if (control === "collection")
    return field(<CollectionControl itemSchema={schema.items} onChange={onChange} value={value} />);
  if (control === "fixed_collection")
    return field(<FixedCollectionControl onChange={onChange} value={value} />);
  if (control === "mapper")
    return field(<MapperControl onChange={onChange} value={value} />);
  return field(
    <NativeReferenceControl
      catalog={referenceCatalog}
      enabled={textReferenceEnabled}
      onChange={onChange}
      schema={schema}
      value={String(value ?? "")}
    />,
  );
}

function NativeReferenceControl({
  value,
  multiline,
  multilineClassName,
  enabled,
  catalog,
  schema,
  onChange,
}: {
  value: string;
  multiline?: boolean;
  multilineClassName?: string;
  enabled?: boolean;
  catalog?: ReferenceCatalog;
  schema: JsonSchemaProperty;
  onChange: (value: string) => void;
}) {
  const control = useRef<HTMLInputElement | HTMLTextAreaElement>(null);
  const insert = (text: string) => {
    const element = control.current;
    const result = insertAtSelection(
      value,
      text,
      element?.selectionStart,
      element?.selectionEnd,
    );
    onChange(result.value);
    requestAnimationFrame(() => {
      element?.focus();
      element?.setSelectionRange(result.cursor, result.cursor);
    });
  };
  return (
    <ReferenceControl
      allowed={allowedNamespaces(schema)}
      catalog={catalog}
      enabled={enabled}
      expectedType={schema.expectedType ?? schema.type}
      onInsert={insert}
    >
      {multiline ? (
        <Textarea
          className={multilineClassName}
          onChange={(event) => onChange(event.target.value)}
          ref={control as React.Ref<HTMLTextAreaElement>}
          value={value}
        />
      ) : (
        <Input
          onChange={(event) => onChange(event.target.value)}
          ref={control as React.Ref<HTMLInputElement>}
          value={value}
        />
      )}
    </ReferenceControl>
  );
}

function ExpressionReferenceControl({
  value,
  enabled,
  catalog,
  schema,
  workflowId,
  onChange,
}: {
  value: string;
  enabled?: boolean;
  catalog?: ReferenceCatalog;
  schema: JsonSchemaProperty;
  workflowId?: string;
  onChange: (value: string) => void;
}) {
  const editor = useRef<CodeEditorHandle>(null);
  return (
    <ReferenceControl
      allowed={allowedNamespaces(schema)}
      catalog={catalog}
      enabled={enabled}
      expectedType={schema.expectedType ?? schema.type}
      onInsert={(text) => editor.current?.insertText(text)}
    >
      <ExpressionEditor
        completionItems={catalog ? completionItems(catalog) : undefined}
        onChange={onChange}
        ref={editor}
        value={value}
        workflowId={workflowId}
      />
    </ReferenceControl>
  );
}

export function ReferenceControl({
  children,
  enabled,
  catalog,
  allowed,
  expectedType,
  onInsert,
}: {
  children: React.ReactNode;
  enabled?: boolean;
  catalog?: ReferenceCatalog;
  allowed: ReferenceNamespace[];
  expectedType?: string;
  onInsert: (value: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const anchor = useRef<HTMLDivElement>(null);
  if (!enabled || !catalog) return children;
  return (
    <div className="relative" onClickCapture={() => setOpen(true)} onFocusCapture={() => setOpen(true)} ref={anchor}>
      {children}
      <ReferencePicker
        allowedNamespaces={allowed}
        catalog={catalog}
        expectedType={expectedType}
        onInsert={onInsert}
        onOpenChange={setOpen}
        open={open}
        anchorRef={anchor}
      />
    </div>
  );
}

function CodeControl({
  language,
  value,
  onChange,
}: {
  language: string;
  value: string;
  onChange: (value: string) => void;
}) {
  const { t } = useTranslation();
  return (
    <div className="overflow-hidden rounded-md border border-border bg-canvas shadow-inner focus-within:border-primary focus-within:ring-2 focus-within:ring-primary/15">
      <div className="flex h-7 items-center justify-between border-b border-border px-2.5 text-[9px] font-medium uppercase tracking-wide text-muted-foreground">
        <span>{language}</span>
        <span>{t("studio.source")}</span>
      </div>
      <CodeEditor
        height="238px"
        language={language}
        onChange={onChange}
        value={value}
      />
    </div>
  );
}

export function StructuredJsonControl({
  value,
  fallback,
  onChange,
  onValidityChange,
  catalog,
  schema,
  enabled,
  localization,
  path = "value",
}: {
  value: unknown;
  fallback: unknown;
  onChange: (value: unknown) => void;
  onValidityChange?: (valid: boolean) => void;
  catalog?: ReferenceCatalog;
  schema: JsonSchemaProperty;
  enabled?: boolean;
  localization?: StructuredFieldLocalization;
  path?: string;
}) {
  useEffect(() => onValidityChange?.(true), [onValidityChange]);
  return <ObjectBuilder catalog={catalog} enabled={enabled} localization={localization} onChange={onChange} path={path} schema={schema} value={value ?? fallback} />;
}

export type StructuredFieldLocalization = {
  label: (path: string) => string;
  description: (path: string) => string | undefined;
  placeholder: (path: string) => string | undefined;
  enumLabel: (path: string, value: string) => string;
};

function ObjectBuilder({ schema, value, onChange, catalog, enabled, path, localization }: { schema: JsonSchemaProperty; value: unknown; onChange: (value: unknown) => void; catalog?: ReferenceCatalog; enabled?: boolean; path: string; localization?: StructuredFieldLocalization }) {
  const { t } = useTranslation();
  if (!schema.type) return <AnyJsonValueBuilder catalog={catalog} enabled={enabled} localization={localization} onChange={onChange} path={path} schema={schema} value={value} />;
  const type = schema.type;
  if (type === 'object') {
    const current = value && typeof value === 'object' && !Array.isArray(value) ? value as Record<string, unknown> : {};
    const properties = schema.properties ?? {};
    const fixedKeys = Object.keys(properties);
    const dynamicKeys = Object.keys(current).filter((name) => !properties[name]);
    const dynamicSchema = typeof schema.additionalProperties === 'object' ? schema.additionalProperties : {};
    const set = (name: string, next: unknown) => onChange({ ...current, [name]: next });
    const remove = (name: string) => { const next = { ...current }; delete next[name]; onChange(next); };
    const rename = (name: string, nextName: string) => { const clean = nextName.trim(); if (!clean || clean === name || current[clean] !== undefined) return; const next = { ...current, [clean]: current[name] }; delete next[name]; onChange(next); };
    const add = () => { let index = dynamicKeys.length + 1; while (current[`field${index}`] !== undefined) index += 1; set(`field${index}`, defaultForSchema(dynamicSchema)); };
    return <div className="space-y-2 rounded-md border border-border/70 p-2" data-testid="json-object">{fixedKeys.map((name) => { const childPath = `${path}.${name}`; const childSchema = properties[name]; const label = localization?.label(childPath) ?? childSchema?.title ?? humanize(name); return <div key={name}>{childSchema?.type !== "boolean" && <span className="mb-1 block text-[10px] font-medium text-muted-foreground">{label}</span>}<ObjectBuilder catalog={catalog} enabled={enabled} localization={localization} onChange={(next) => set(name, next)} path={childPath} schema={childSchema} value={current[name]} />{localization?.description(childPath) && <p className="mt-1 text-[10px] text-muted-foreground">{localization.description(childPath)}</p>}</div>; })}{schema.additionalProperties !== false && dynamicKeys.map((name) => <div className="space-y-2 rounded-md border border-border/60 p-2" data-testid="json-object-field" key={name}><div className="flex items-center gap-2"><Input aria-label={t('studio.key')} className="min-w-0 flex-1" defaultValue={name} onBlur={(event) => rename(name, event.target.value)} /><Button aria-label={t('studio.removeField', { key: name })} onClick={() => remove(name)} size="icon" variant="ghost"><Trash2 className="size-3.5" /></Button></div><ObjectBuilder catalog={catalog} enabled={enabled} localization={localization} onChange={(next) => set(name, next)} path={`${path}.${name}`} schema={dynamicSchema} value={current[name]} /></div>)}{schema.additionalProperties !== false && <Button aria-label={t('studio.addField')} onClick={add} size="sm" variant="ghost"><Plus className="size-3.5" />{t('studio.addField')}</Button>}</div>;
  }
  if (type === 'array') {
    const items = Array.isArray(value) ? value : [];
    return <div className="space-y-2" data-testid="json-array">{items.map((item, index) => <div className="space-y-1 rounded-md border border-border/60 p-2" data-testid="json-array-item" key={index}><div className="flex items-center justify-between"><span className="text-[10px] text-muted-foreground">#{index + 1}</span><Button aria-label={t('studio.remove', { index: index + 1 })} onClick={() => onChange(items.filter((_, currentIndex) => currentIndex !== index))} size="icon" variant="ghost"><Trash2 className="size-3.5" /></Button></div><ObjectBuilder catalog={catalog} enabled={enabled} localization={localization} onChange={(next) => onChange(items.map((current, currentIndex) => currentIndex === index ? next : current))} path={`${path}[]`} schema={schema.items ?? {}} value={item} /></div>)}<Button aria-label={t('studio.addItem')} onClick={() => onChange([...items, defaultForSchema(schema.items)])} size="sm" variant="secondary"><Plus className="size-3.5" />{t('studio.addItem')}</Button></div>;
  }
  const label = localization?.label(path) ?? schema.title ?? humanize(path.split('.').at(-1)?.replace('[]', '') ?? path);
  if (type === 'boolean') return <label className="flex items-center gap-2 text-xs"><input checked={Boolean(value)} className="size-4 accent-primary" onChange={(event) => onChange(event.target.checked)} type="checkbox" />{label}</label>;
  if (schema.enum?.length) return <Select aria-label={label} className="w-full" onValueChange={(next) => onChange(schema.enum?.find((option) => String(option) === next) ?? next)} options={schema.enum.map((option) => ({ value: String(option), label: localization?.enumLabel(path, String(option)) ?? String(option) }))} value={value === undefined ? "" : String(value)} />;
  return <ReferenceControl allowed={allowedNamespaces(schema)} catalog={catalog} enabled={(enabled || schema.templatable) && schema.type === 'string'} onInsert={(reference) => onChange(`${String(value ?? '')}${reference}`)}><Input aria-label={label || t('studio.value')} min={schema.minimum} onChange={(event) => onChange(type === 'number' || type === 'integer' ? Number(event.target.value) : event.target.value)} placeholder={localization?.placeholder(path)} type={type === 'number' || type === 'integer' ? 'number' : 'text'} value={value === undefined || value === null ? '' : String(value)} /></ReferenceControl>;
}

function AnyJsonValueBuilder({ schema, value, onChange, catalog, enabled, path = "value", localization }: { schema: JsonSchemaProperty; value: unknown; onChange: (value: unknown) => void; catalog?: ReferenceCatalog; enabled?: boolean; path?: string; localization?: StructuredFieldLocalization }) {
  const { t } = useTranslation();
  const type = jsonValueType(value);
  const typedSchema = { ...schema, type: type === 'null' ? 'string' : type } as JsonSchemaProperty;
  return <div className="space-y-1" data-testid="json-any-value"><Select aria-label={t('studio.jsonValueType')} className="w-full min-w-0" onValueChange={(next) => onChange(defaultForSchema({ type: next }))} options={JSON_VALUE_TYPES.map((item) => ({ value: item, label: t(`studio.schemaTypes.${item}`, item) }))} value={type === 'null' ? 'string' : type} /><ObjectBuilder catalog={catalog} enabled={enabled} localization={localization} onChange={onChange} path={path} schema={typedSchema} value={value === null || value === undefined ? defaultForSchema(typedSchema) : value} /></div>;
}

const JSON_VALUE_TYPES = ['string', 'number', 'boolean', 'object', 'array'] as const;
function jsonValueType(value: unknown): string { if (Array.isArray(value)) return 'array'; if (value && typeof value === 'object') return 'object'; if (typeof value === 'number') return 'number'; if (typeof value === 'boolean') return 'boolean'; return 'string'; }

function defaultForSchema(schema?: JsonSchemaProperty): unknown { if (schema?.default !== undefined) return schema.default; if (schema?.type === 'object') return {}; if (schema?.type === 'array') return []; if (schema?.type === 'boolean') return false; if (schema?.type === 'number' || schema?.type === 'integer') return 0; return ''; }

function CollectionControl({
  value,
  itemSchema,
  onChange,
}: {
  value: unknown;
  itemSchema?: JsonSchemaProperty;
  onChange: (value: unknown[]) => void;
}) {
  const { t } = useTranslation();
  const items = Array.isArray(value) ? value : [];
  const update = (index: number, next: unknown) =>
    onChange(items.map((item, current) => (current === index ? next : item)));
  return (
    <div className="space-y-2">
      {items.map((item, index) => (
        <div className="flex items-start gap-2" key={index}>
          <ObjectBuilder
            onChange={(next) => update(index, next)}
            path={`items[]`}
            schema={itemSchema ?? {}}
            value={item}
          />
          <Button
            aria-label={t("studio.remove", { index: index + 1 })}
            onClick={() =>
              onChange(items.filter((_, current) => current !== index))
            }
            size="icon"
            variant="ghost"
          >
            <Trash2 className="size-3.5" />
          </Button>
        </div>
      ))}
      <Button
        onClick={() =>
          onChange([...items, itemSchema?.type === "object" ? {} : ""])
        }
        size="sm"
        variant="secondary"
      >
        <Plus className="size-3.5" />
        {t("studio.addItem")}
      </Button>
    </div>
  );
}

function FixedCollectionControl({
  value,
  onChange,
}: {
  value: unknown;
  onChange: (value: Record<string, unknown>) => void;
}) {
  const { t } = useTranslation();
  const entries = Object.entries(
    value && typeof value === "object" && !Array.isArray(value)
      ? (value as Record<string, unknown>)
      : {},
  );
  const replace = (index: number, key: string, next: unknown) =>
    onChange(
      Object.fromEntries(
        entries.map(([currentKey, currentValue], current) =>
          current === index ? [key, next] : [currentKey, currentValue],
        ),
      ),
    );
  return (
    <div className="space-y-2">
      {entries.map(([key, item], index) => (
        <div
          className="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)_32px] gap-2"
          key={`${key}-${index}`}
        >
          <Input
            aria-label={t("studio.key")}
            onChange={(event) => replace(index, event.target.value, item)}
            value={key}
          />
          <AnyJsonValueBuilder onChange={(next) => replace(index, key, next)} schema={{}} value={item} />
          <Button
            aria-label={t("studio.removeField", { key })}
            onClick={() =>
              onChange(
                Object.fromEntries(
                  entries.filter((_, current) => current !== index),
                ),
              )
            }
            size="icon"
            variant="ghost"
          >
            <Trash2 className="size-3.5" />
          </Button>
        </div>
      ))}
      <Button
        onClick={() =>
          onChange({
            ...Object.fromEntries(entries),
            [`field${entries.length + 1}`]: "",
          })
        }
        size="sm"
        variant="secondary"
      >
        <Plus className="size-3.5" />
        {t("studio.addField")}
      </Button>
    </div>
  );
}

function MapperControl({
  value,
  onChange,
}: {
  value: unknown;
  onChange: (value: Record<string, unknown>) => void;
}) {
  const { t } = useTranslation();
  const entries = Object.entries(
    value && typeof value === "object" && !Array.isArray(value)
      ? (value as Record<string, unknown>)
      : {},
  );
  const replace = (index: number, key: string, next: unknown) =>
    onChange(
      Object.fromEntries(
        entries.map(([currentKey, currentValue], current) =>
          current === index ? [key, next] : [currentKey, currentValue],
        ),
      ),
    );
  return (
    <div className="space-y-2" data-testid="mapper-control">
      {entries.map(([key, item], index) => (
        <div
          className="grid grid-cols-[minmax(0,1fr)_minmax(0,1.35fr)_32px] gap-2"
          key={`${key}-${index}`}
        >
          <Input
            aria-label={t("studio.key")}
            onChange={(event) => replace(index, event.target.value, item)}
            value={key}
          />
          <AnyJsonValueBuilder onChange={(next) => replace(index, key, next)} schema={{}} value={item} />
          <Button
            aria-label={t("studio.removeField", { key })}
            onClick={() =>
              onChange(
                Object.fromEntries(
                  entries.filter((_, current) => current !== index),
                ),
              )
            }
            size="icon"
            variant="ghost"
          >
            <Trash2 className="size-3.5" />
          </Button>
        </div>
      ))}
      <Button
        onClick={() =>
          onChange({
            ...Object.fromEntries(entries),
            [`field${entries.length + 1}`]: "",
          })
        }
        size="sm"
        variant="secondary"
      >
        <Plus className="size-3.5" />
        {t("studio.addField")}
      </Button>
    </div>
  );
}

function UnsupportedControl({
  label,
  control,
}: {
  label: string;
  control?: string;
}) {
  const { t } = useTranslation();
  return (
    <div className="border border-danger/40 bg-danger/5 p-3 text-xs text-danger">
      <div className="flex items-center gap-2 font-medium">
        <AlertTriangle className="size-3.5" />
        {label}
      </div>
      <p className="mt-1 text-[10px]">
        {t("studio.unsupported", { control: control ?? "missing" })}
      </p>
    </div>
  );
}
function Field({
  children,
  label,
  unit,
  required,
  error,
  testId,
}: {
  children: React.ReactNode;
  label: string;
  unit?: string;
  required?: boolean;
  error?: string;
  testId?: string;
}) {
  return (
    <div className="block text-xs" data-testid={testId}>
      <span className="mb-1.5 flex items-center justify-between gap-2 text-muted-foreground">
        <span>{label}</span>
        {unit && <span className="rounded bg-muted px-1.5 py-0.5 text-[10px] font-medium text-muted-foreground">{unit}</span>}
        {required && <span className="ml-1 text-danger">*</span>}
      </span>
      {children}
      {error && (
        <span className="mt-1 block text-[10px] text-danger">{error}</span>
      )}
    </div>
  );
}
const humanize = (value: string) =>
  value
    .replace(/([a-z])([A-Z])/g, "$1 $2")
    .replaceAll("_", " ")
    .replace(/^./, (letter) => letter.toUpperCase());
const codeLanguage = (runner: unknown) =>
  runner === "javascript"
    ? "javascript"
    : runner === "shell"
      ? "shell"
      : runner === "browser"
        ? "typescript"
        : "python";
const inferredUnit = (name: string) => {
  if (name.endsWith("Ms")) return "milliseconds";
  if (name.includes("Tokens")) return "tokens";
  if (name.includes("Calls") || name.includes("Iterations")) return "calls";
  if (name.includes("CostMicros")) return "micros";
  if (name === "maxItems" || name === "batchSize") return "items";
  return undefined;
};
const selectOption = (item: unknown, localizedLabel?: string) =>
  item !== null &&
  typeof item === "object" &&
  "value" in item &&
  "label" in item
    ? { value: String(item.value), label: localizedLabel ?? String(item.label) }
    : { value: String(item), label: localizedLabel ?? String(item) };
const allowedNamespaces = (schema: JsonSchemaProperty): ReferenceNamespace[] =>
  (schema.allowedNamespaces ?? ["inputs", "outputs", "contexts"]).filter(
    (namespace): namespace is ReferenceNamespace =>
      namespace === "inputs" ||
      namespace === "outputs" ||
      namespace === "contexts" ||
      namespace === "item" ||
      namespace === "execution" ||
      namespace === "loop",
  );
function completionItems(catalog: ReferenceCatalog) {
  return Object.values(catalog)
    .flatMap((entries) => flattenReferences(entries))
    .filter((entry) => entry.expression && !entry.sensitive)
    .map((entry) => ({
      label: entry.path,
      insertText: entry.expression!,
      detail: entry.type,
    }));
}
function flattenReferences(
  entries: import("../model/types").ReferenceEntry[],
): import("../model/types").ReferenceEntry[] {
  return entries.flatMap((entry) => [
    entry,
    ...flattenReferences(entry.children),
  ]);
}
export const SUPPORTED_CONTROLS = new Set([
  "text",
  "textarea",
  "number",
  "boolean",
  "select",
  "provider_options",
  "collection",
  "fixed_collection",
  "mapper",
  "expression",
  "prompt",
  "json",
  "code",
]);
