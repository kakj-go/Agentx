import { Braces, X } from "lucide-react";
import { useRef, useState } from "react";
import type { LexicalEditor } from "lexical";

import { Button } from "../../../shared/ui/button";
import type {
  JsonSchemaProperty,
  ReferenceBinding,
  ReferenceCatalog,
  ReferenceNamespace,
  TemplateBinding,
  InputBinding,
  ValueSelector,
} from "../model/types";
import { ReferencePicker } from "./reference-picker/reference-picker";
import {
  insertVariable,
  selectorDisplayColor,
  selectorDisplayLabel,
  VariableTokenEditor,
} from "./variable-token-editor";
import { bindingToEditorValue, StructuredTemplateEditor } from "./structured-template-editor";

const DEFAULT_NAMESPACES: ReferenceNamespace[] = ["inputs", "outputs", "contexts"];

export function ReferenceInput({
  value,
  onChange,
  catalog,
  expectedSchema,
  allowedNamespaces = DEFAULT_NAMESPACES,
  placeholder = "选择变量",
  onClear,
}: {
  value?: ReferenceBinding;
  onChange: (value: ReferenceBinding) => void;
  catalog?: ReferenceCatalog;
  expectedSchema?: JsonSchemaProperty;
  allowedNamespaces?: ReferenceNamespace[];
  placeholder?: string;
  onClear?: () => void;
}) {
  const [open, setOpen] = useState(false);
  const anchor = useRef<HTMLDivElement>(null);
  const label = value ? selectorDisplayLabel(value.selector, catalog) : placeholder;
  const color = value ? selectorDisplayColor(value.selector, catalog) : undefined;
  return (
    <div className="relative" ref={anchor}>
      <div className="flex min-h-[30px] items-center gap-1 rounded-md border border-border bg-surface px-1.5 focus-within:border-primary focus-within:ring-2 focus-within:ring-primary/15" data-testid="reference-input">
        <button className="flex min-w-0 flex-1 items-center gap-1.5 px-1 text-left text-xs" onClick={() => setOpen(true)} type="button">
          {value ? <>
            <span className="size-1.5 shrink-0 rounded-full" style={{ backgroundColor: color }} />
            <span className="min-w-0 truncate font-medium text-primary">{label}</span>
          </> : <>
            <Braces className="size-3.5 shrink-0 text-muted-foreground" />
            <span className="text-muted-foreground">{placeholder}</span>
          </>}
        </button>
        {value && onClear && <Button aria-label="Clear variable" className="!size-7" onClick={onClear} size="icon" variant="ghost"><X className="size-3" /></Button>}
      </div>
      {catalog && <ReferencePicker
        allowedNamespaces={allowedNamespaces}
        anchorRef={anchor}
        catalog={catalog}
        expectedSchema={expectedSchema}
        onInsert={(selector) => onChange(referenceBinding(selector))}
        onOpenChange={setOpen}
        open={open}
      />}
    </div>
  );
}

export function SmartInput({
  value,
  onChange,
  catalog,
  expectedSchema,
  allowedNamespaces = DEFAULT_NAMESPACES,
  multiline = false,
}: {
  value: InputBinding;
  onChange: (value: InputBinding) => void;
  catalog?: ReferenceCatalog;
  expectedSchema?: JsonSchemaProperty;
  allowedNamespaces?: ReferenceNamespace[];
  multiline?: boolean;
}) {
  const [open, setOpen] = useState(false);
  const [trigger, setTrigger] = useState<"{{" | "/">();
  const anchor = useRef<HTMLDivElement>(null);
  const editor = useRef<LexicalEditor | null>(null);
  const lastEmitted = useRef(value);
  lastEmitted.current = value;
  const emitChange = (next: InputBinding) => {
    if (JSON.stringify(next) === JSON.stringify(lastEmitted.current)) return;
    lastEmitted.current = next;
    onChange(next);
  };
  const structured = isStructuredInput(value) || schemaType(expectedSchema) === "array" || schemaType(expectedSchema) === "object";
  if (structured) return <StructuredTemplateEditor allowedNamespaces={allowedNamespaces} catalog={catalog} expectedSchema={expectedSchema} onChange={onChange} value={normalizeStructuredInput(value)} />;
  const display = scalarEditorValue(value);
  const empty = display.segments.length === 0;
  return <div className="relative" onClickCapture={(event) => {
    if (empty && event.target instanceof HTMLElement && event.target.closest('[contenteditable="true"]')) {
      setTrigger(undefined);
      setOpen(true);
    }
  }} ref={anchor}>
    <VariableTokenEditor
      catalog={catalog}
      multiline={multiline}
      onChange={(source) => {
        const lastText = [...source.segments].reverse().find((segment) => segment.kind === "text");
        if (lastText?.kind === "text") {
          const nextTrigger = lastText.text.endsWith("{{") ? "{{" : lastText.text.endsWith("/") ? "/" : undefined;
          if (nextTrigger) {
            setTrigger(nextTrigger);
            setOpen(true);
          }
        }
        emitChange(normalizeScalarEditorValue(source, expectedSchema));
      }}
      onEditorReady={(next) => { editor.current = next; }}
      value={display}
    />
    <Button aria-label="Insert variable" className="absolute right-1 top-0.5 !size-7" onClick={() => { setTrigger(undefined); setOpen(true); }} size="icon" variant="ghost"><Braces className="size-3.5" /></Button>
    {catalog && <ReferencePicker
      allowedNamespaces={allowedNamespaces}
      anchorRef={anchor}
      catalog={catalog}
      expectedSchema={expectedSchema}
      onInsert={(selector) => {
        if (empty || !editor.current) {
          emitChange(referenceBinding(selector));
          return;
        }
        insertVariable(editor.current, selector, selectorDisplayLabel(selector, catalog), selectorDisplayColor(selector, catalog), trigger);
        setTrigger(undefined);
      }}
      onOpenChange={(next) => { setOpen(next); if (!next) setTrigger(undefined); }}
      open={open}
    />}
  </div>;
}

export function TemplateInput(props: {
  value: InputBinding;
  onChange: (value: InputBinding) => void;
  catalog?: ReferenceCatalog;
  allowedNamespaces?: ReferenceNamespace[];
  multiline?: boolean;
}) {
  return <SmartInput {...props} expectedSchema={{ type: "string" }} />;
}

function scalarEditorValue(value: InputBinding): TemplateBinding {
  if (value.kind === "template") return value;
  if (value.kind === "reference") return { kind: "template", segments: [{ kind: "reference", selector: value.selector, missingPolicy: value.missingPolicy }] };
  if (value.kind === "literal") return { kind: "template", segments: value.value === "" || value.value === undefined ? [] : [{ kind: "text", text: literalText(value.value) }] };
  return bindingToEditorValue(value);
}

function normalizeScalarEditorValue(value: TemplateBinding, expectedSchema?: JsonSchemaProperty): InputBinding {
  if (value.segments.length === 1 && value.segments[0]?.kind === "reference") {
    const reference = value.segments[0];
    return { kind: "reference", selector: reference.selector, missingPolicy: reference.missingPolicy };
  }
  if (value.segments.every((segment) => segment.kind === "text")) {
    const text = value.segments.map((segment) => segment.kind === "text" ? segment.text : "").join("");
    return inputBindingFromValue(parseLiteral(text, expectedSchema));
  }
  return value;
}

function normalizeStructuredInput(value: InputBinding): InputBinding {
  if (value.kind !== "literal") return value;
  return inputBindingFromValue(value.value);
}

function isStructuredInput(value: InputBinding) {
  return value.kind === "array" || value.kind === "object" || value.kind === "literal" && Boolean(value.value && typeof value.value === "object");
}

export const referenceBinding = (selector: ValueSelector): ReferenceBinding => ({
  kind: "reference",
  selector,
  missingPolicy: { kind: "error" },
});

export function asInputBinding(value: unknown): InputBinding {
  if (value && typeof value === "object" && "kind" in value) {
    const candidate = value as { kind?: unknown; selector?: unknown; missingPolicy?: unknown; value?: unknown };
    if (candidate.kind === "reference" && candidate.selector) return value as ReferenceBinding;
    if (["literal", "template", "array", "object"].includes(String(candidate.kind))) return value as InputBinding;
  }
  return inputBindingFromValue(value);
}

export function asReferenceBinding(value: unknown): ReferenceBinding | undefined {
  const binding = asInputBinding(value);
  return binding.kind === "reference" ? binding : undefined;
}

export function parseLiteral(value: string, expectedSchema?: JsonSchemaProperty): unknown {
  if (value === "") return "";
  if (schemaType(expectedSchema) === "string") return value;
  try {
    return JSON.parse(value) as unknown;
  } catch {
    return value;
  }
}

export function inputBindingFromValue(value: unknown): InputBinding {
  if (Array.isArray(value)) return { kind: "array", items: value.map(inputBindingFromValue) };
  if (value && typeof value === "object") return {
    kind: "object",
    fields: Object.fromEntries(Object.entries(value).map(([name, child]) => [name, inputBindingFromValue(child)])),
  };
  return { kind: "literal", value };
}

export function literalText(value: unknown): string {
  if (typeof value === "string") return value;
  return JSON.stringify(value) ?? "";
}

function schemaType(schema?: JsonSchemaProperty): string {
  const types = schema?.type ? Array.isArray(schema.type) ? schema.type : [schema.type] : [];
  return types.find((type) => type !== "null") ?? types[0] ?? "unknown";
}
