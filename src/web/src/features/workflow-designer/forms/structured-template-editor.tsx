import JSON5 from "json5";
import type { LexicalEditor } from "lexical";
import { Braces } from "lucide-react";
import { useMemo, useRef, useState } from "react";

import { Button } from "../../../shared/ui/button";
import type {
  InputBinding,
  InputTemplateSegment,
  JsonSchemaProperty,
  ReferenceCatalog,
  ReferenceNamespace,
  ValueSelector,
} from "../model/types";
import { ReferencePicker } from "./reference-picker/reference-picker";
import {
  insertVariable,
  selectorDisplayColor,
  selectorDisplayLabel,
  VariableTokenEditor,
} from "./variable-token-editor";

const VALUE_MARKER = "__AGENTX_VALUE_REFERENCE_";
const TEMPLATE_MARKER = "__AGENTX_TEMPLATE_REFERENCE_";

export function StructuredTemplateEditor({
  value,
  onChange,
  catalog,
  allowedNamespaces,
  expectedSchema,
}: {
  value: InputBinding;
  onChange: (value: InputBinding) => void;
  catalog?: ReferenceCatalog;
  allowedNamespaces: ReferenceNamespace[];
  expectedSchema?: JsonSchemaProperty;
}) {
  const [open, setOpen] = useState(false);
  const [trigger, setTrigger] = useState<"{{" | "/">();
  const [error, setError] = useState<string>();
  const anchor = useRef<HTMLDivElement>(null);
  const editor = useRef<LexicalEditor | null>(null);
  const display = useMemo(() => bindingToEditorValue(value), [value]);
  const empty = value.kind === "literal" && (value.value === undefined || value.value === "");
  return <div className="relative" onClickCapture={(event) => {
    if (empty && event.target instanceof HTMLElement && event.target.closest('[contenteditable="true"]')) {
      setTrigger(undefined);
      setOpen(true);
    }
  }} ref={anchor}>
    <VariableTokenEditor
      catalog={catalog}
      multiline
      onChange={(source) => {
        const lastText = [...source.segments].reverse().find((segment) => segment.kind === "text");
        if (lastText?.kind === "text") {
          const nextTrigger = lastText.text.endsWith("{{") ? "{{" : lastText.text.endsWith("/") ? "/" : undefined;
          if (nextTrigger) {
            setTrigger(nextTrigger);
            setOpen(true);
          }
        }
        try {
          const parsed = parseStructuredEditorValue(source);
          setError(undefined);
          onChange(parsed);
        } catch (cause) {
          setError(cause instanceof Error ? cause.message : String(cause));
        }
      }}
      onEditorReady={(next) => { editor.current = next; }}
      value={display}
    />
    <Button aria-label="Insert variable" className="absolute right-1 top-0.5 !size-7" onClick={() => { setTrigger(undefined); setOpen(true); }} size="icon" variant="ghost"><Braces className="size-3.5" /></Button>
    {error && <p className="mt-1 text-[10px] text-danger" role="alert">{error}</p>}
    {catalog && <ReferencePicker
      allowedNamespaces={allowedNamespaces}
      anchorRef={anchor}
      catalog={catalog}
      expectedSchema={expectedSchema}
      onInsert={(selector) => {
        if (empty || !editor.current) {
          onChange(reference(selector));
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

export function parseStructuredEditorValue(source: Extract<InputBinding, { kind: "template" }>): InputBinding {
  const references: Extract<InputTemplateSegment, { kind: "reference" }>[] = [];
  let text = "";
  let quote: "'" | '"' | undefined;
  let escaped = false;
  for (const segment of source.segments) {
    if (segment.kind === "reference") {
      const index = references.push(segment) - 1;
      text += quote ? `${TEMPLATE_MARKER}${index}__` : JSON.stringify(`${VALUE_MARKER}${index}__`);
      continue;
    }
    text += segment.text;
    for (const character of segment.text) {
      if (escaped) {
        escaped = false;
        continue;
      }
      if (character === "\\" && quote) {
        escaped = true;
      } else if (quote && character === quote) {
        quote = undefined;
      } else if (!quote && (character === '"' || character === "'")) {
        quote = character;
      }
    }
  }
  const parsed = JSON5.parse(text) as unknown;
  return parsedValueToBinding(parsed, references);
}

function parsedValueToBinding(
  value: unknown,
  references: Extract<InputTemplateSegment, { kind: "reference" }>[],
): InputBinding {
  if (typeof value === "string") {
    const standalone = markerIndex(value, VALUE_MARKER);
    if (standalone !== undefined && value === `${VALUE_MARKER}${standalone}__`) {
      const reference = references[standalone];
      if (!reference) throw new Error("Variable placeholder is invalid");
      return { kind: "reference", selector: reference.selector, missingPolicy: reference.missingPolicy };
    }
    if (value.includes(TEMPLATE_MARKER)) {
      const segments: InputTemplateSegment[] = [];
      const pattern = new RegExp(`${TEMPLATE_MARKER}(\\d+)__`, "g");
      let cursor = 0;
      for (const match of value.matchAll(pattern)) {
        if (match.index > cursor) segments.push({ kind: "text", text: value.slice(cursor, match.index) });
        const reference = references[Number(match[1])];
        if (!reference) throw new Error("Variable placeholder is invalid");
        segments.push(reference);
        cursor = (match.index ?? 0) + match[0].length;
      }
      if (cursor < value.length) segments.push({ kind: "text", text: value.slice(cursor) });
      return { kind: "template", segments };
    }
    return { kind: "literal", value };
  }
  if (Array.isArray(value)) return { kind: "array", items: value.map((item) => parsedValueToBinding(item, references)) };
  if (value && typeof value === "object") return {
    kind: "object",
    fields: Object.fromEntries(Object.entries(value).map(([name, child]) => [name, parsedValueToBinding(child, references)])),
  };
  return { kind: "literal", value };
}

function markerIndex(value: string, marker: string) {
  const match = value.match(new RegExp(`^${marker}(\\d+)__$`));
  return match ? Number(match[1]) : undefined;
}

export function bindingToEditorValue(value: InputBinding): Extract<InputBinding, { kind: "template" }> {
  const segments: InputTemplateSegment[] = [];
  appendBinding(value, segments, 0);
  return { kind: "template", segments };
}

function appendBinding(value: InputBinding, output: InputTemplateSegment[], depth: number) {
  switch (value.kind) {
    case "literal":
      if (value.value !== undefined && value.value !== "") pushText(output, JSON.stringify(value.value, null, 2) ?? "null");
      break;
    case "reference":
      output.push({ kind: "reference", selector: value.selector, missingPolicy: value.missingPolicy });
      break;
    case "template":
      pushText(output, '"');
      for (const segment of value.segments) {
        if (segment.kind === "text") pushText(output, JSON.stringify(segment.text).slice(1, -1));
        else output.push(segment);
      }
      pushText(output, '"');
      break;
    case "array":
      if (!value.items.length) { pushText(output, "[]"); break; }
      pushText(output, "[\n");
      value.items.forEach((item, index) => {
        pushText(output, "  ".repeat(depth + 1));
        appendBinding(item, output, depth + 1);
        pushText(output, index + 1 === value.items.length ? "\n" : ",\n");
      });
      pushText(output, `${"  ".repeat(depth)}]`);
      break;
    case "object": {
      const entries = Object.entries(value.fields);
      if (!entries.length) { pushText(output, "{}"); break; }
      pushText(output, "{\n");
      entries.forEach(([name, child], index) => {
        pushText(output, `${"  ".repeat(depth + 1)}${JSON.stringify(name)}: `);
        appendBinding(child, output, depth + 1);
        pushText(output, index + 1 === entries.length ? "\n" : ",\n");
      });
      pushText(output, `${"  ".repeat(depth)}}`);
      break;
    }
  }
}

function pushText(output: InputTemplateSegment[], text: string) {
  const last = output.at(-1);
  if (last?.kind === "text") last.text += text;
  else output.push({ kind: "text", text });
}

export function reference(selector: ValueSelector): InputBinding {
  return { kind: "reference", selector, missingPolicy: { kind: "error" } };
}
