import { Braces, Plus, Trash2 } from "lucide-react";
import { useRef, useState } from "react";

import { Button } from "../../../shared/ui/button";
import { Input } from "../../../shared/ui/input";
import { Select } from "../../../shared/ui/select";
import type {
  ExpressionNode,
  ReferenceCatalog,
  ReferenceNamespace,
  ValueSelector,
} from "../model/types";
import { ReferencePicker } from "./reference-picker/reference-picker";
import { selectorDisplayLabel } from "./variable-token-editor";

const KINDS = ["literal", "reference", "unary", "binary", "conditional", "call", "array", "object"] as const;
const BINARY = ["eq", "ne", "gt", "gte", "lt", "lte", "add", "subtract", "multiply", "divide", "modulo", "and", "or", "in"] as const;
const FUNCTIONS = ["contains", "starts_with", "ends_with", "matches", "size", "string", "int", "uint", "double", "timestamp", "duration", "max", "min"] as const;

export function ExpressionBuilder({
  value,
  onChange,
  catalog,
  allowed,
}: {
  value: ExpressionNode;
  onChange: (value: ExpressionNode) => void;
  catalog?: ReferenceCatalog;
  allowed: ReferenceNamespace[];
}) {
  return (
    <div className="space-y-2 rounded-md border border-border/70 bg-canvas/40 p-2" data-testid="expression-builder">
      <ExpressionNodeEditor allowed={allowed} catalog={catalog} depth={0} onChange={onChange} value={value} />
    </div>
  );
}

function ExpressionNodeEditor({
  value,
  onChange,
  catalog,
  allowed,
  depth,
}: {
  value: ExpressionNode;
  onChange: (value: ExpressionNode) => void;
  catalog?: ReferenceCatalog;
  allowed: ReferenceNamespace[];
  depth: number;
}) {
  const setKind = (kind: string) => onChange(defaultNode(kind));
  const child = (node: ExpressionNode, change: (next: ExpressionNode) => void) => (
    <ExpressionNodeEditor allowed={allowed} catalog={catalog} depth={depth + 1} onChange={change} value={node} />
  );
  return (
    <div className={depth ? "space-y-2 border-l border-border pl-2" : "space-y-2"}>
      <Select
        aria-label="Expression type"
        className="w-full"
        onValueChange={setKind}
        options={KINDS.map((kind) => ({ value: kind, label: kindLabel(kind) }))}
        value={value.kind}
      />
      {value.kind === "literal" && (
        <Input
          aria-label="Expression"
          onChange={(event) => onChange({ kind: "literal", value: parseLiteral(event.target.value) })}
          value={literalText(value.value)}
        />
      )}
      {value.kind === "reference" && (
        <ReferenceOperand allowed={allowed} catalog={catalog} onChange={(selector) => onChange({ ...value, selector })} selector={value.selector} />
      )}
      {value.kind === "unary" && <>
        <Select aria-label="Unary operator" className="w-full" onValueChange={(operator) => onChange({ ...value, operator: operator as "not" | "negate" })} options={[{ value: "not", label: "NOT" }, { value: "negate", label: "−" }]} value={value.operator} />
        {child(value.operand, (operand) => onChange({ ...value, operand }))}
      </>}
      {value.kind === "binary" && <>
        <Select aria-label="Binary operator" className="w-full" onValueChange={(operator) => onChange({ ...value, operator: operator as typeof value.operator })} options={BINARY.map((operator) => ({ value: operator, label: binaryLabel(operator) }))} value={value.operator} />
        <div className="space-y-2 rounded border border-border/60 p-2">
          <span className="text-[10px] text-muted-foreground">左值</span>
          {child(value.left, (left) => onChange({ ...value, left }))}
          <span className="text-[10px] text-muted-foreground">右值</span>
          {child(value.right, (right) => onChange({ ...value, right }))}
        </div>
      </>}
      {value.kind === "conditional" && <div className="space-y-2">
        <ExpressionSection label="条件">{child(value.condition, (condition) => onChange({ ...value, condition }))}</ExpressionSection>
        <ExpressionSection label="成立时">{child(value.thenValue, (thenValue) => onChange({ ...value, thenValue }))}</ExpressionSection>
        <ExpressionSection label="否则">{child(value.elseValue, (elseValue) => onChange({ ...value, elseValue }))}</ExpressionSection>
      </div>}
      {value.kind === "call" && <>
        <Select aria-label="Function" className="w-full" onValueChange={(fn) => onChange({ ...value, function: fn })} options={FUNCTIONS.map((fn) => ({ value: fn, label: `${fn}()` }))} value={value.function} />
        {value.arguments.map((argument, index) => <ExpressionRow key={index} onRemove={() => onChange({ ...value, arguments: value.arguments.filter((_, current) => current !== index) })}>{child(argument, (next) => onChange({ ...value, arguments: value.arguments.map((item, current) => current === index ? next : item) }))}</ExpressionRow>)}
        <AddButton label="添加参数" onClick={() => onChange({ ...value, arguments: [...value.arguments, literalNode()] })} />
      </>}
      {value.kind === "array" && <>
        {value.items.map((item, index) => <ExpressionRow key={index} onRemove={() => onChange({ ...value, items: value.items.filter((_, current) => current !== index) })}>{child(item, (next) => onChange({ ...value, items: value.items.map((entry, current) => current === index ? next : entry) }))}</ExpressionRow>)}
        <AddButton label="添加数组项" onClick={() => onChange({ ...value, items: [...value.items, literalNode()] })} />
      </>}
      {value.kind === "object" && <>
        {Object.entries(value.fields).map(([key, item]) => <ExpressionRow key={key} onRemove={() => { const fields = { ...value.fields }; delete fields[key]; onChange({ ...value, fields }); }}><Input aria-label="Object field" onChange={(event) => { const next = event.target.value.trim(); if (!next || next === key || value.fields[next]) return; const fields = { ...value.fields, [next]: item }; delete fields[key]; onChange({ ...value, fields }); }} value={key} />{child(item, (next) => onChange({ ...value, fields: { ...value.fields, [key]: next } }))}</ExpressionRow>)}
        <AddButton label="添加对象字段" onClick={() => { let index = Object.keys(value.fields).length + 1; while (value.fields[`field${index}`]) index += 1; onChange({ ...value, fields: { ...value.fields, [`field${index}`]: literalNode() } }); }} />
      </>}
    </div>
  );
}

function ReferenceOperand({ selector, onChange, catalog, allowed }: { selector: ValueSelector; onChange: (selector: ValueSelector) => void; catalog?: ReferenceCatalog; allowed: ReferenceNamespace[] }) {
  const [open, setOpen] = useState(false);
  const anchor = useRef<HTMLDivElement>(null);
  return <div className="relative space-y-1" ref={anchor}>
    <button className="flex min-h-9 w-full items-center gap-1 rounded-md border border-border bg-surface px-2 text-left text-xs text-primary" onClick={() => setOpen(true)} type="button"><Braces className="size-3" /><span className="truncate">{selectorLabel(selector, catalog)}</span></button>
    <Input aria-label="Reference path" onChange={(event) => onChange({ ...selector, path: parsePath(event.target.value) })} placeholder="field.nested.0" value={selector.path.join('.')} />
    {catalog && <ReferencePicker allowedNamespaces={allowed} catalog={catalog} onInsert={(next) => { onChange(next); setOpen(false); }} onOpenChange={setOpen} open={open} anchorRef={anchor} />}
  </div>;
}

function ExpressionSection({ label, children }: { label: string; children: React.ReactNode }) {
  return <div className="space-y-1 rounded border border-border/60 p-2"><span className="text-[10px] font-medium text-muted-foreground">{label}</span>{children}</div>;
}

function ExpressionRow({ children, onRemove }: { children: React.ReactNode; onRemove: () => void }) {
  return <div className="grid grid-cols-[minmax(0,1fr)_28px] gap-1 rounded border border-border/60 p-1"><div className="space-y-1">{children}</div><Button aria-label="Remove expression" onClick={onRemove} size="icon" variant="ghost"><Trash2 className="size-3" /></Button></div>;
}

function AddButton({ label, onClick }: { label: string; onClick: () => void }) {
  return <Button onClick={onClick} size="sm" variant="ghost"><Plus className="size-3" />{label}</Button>;
}

const literalNode = (): ExpressionNode => ({ kind: "literal", value: "" });
function defaultNode(kind: string): ExpressionNode {
  if (kind === "reference") return { kind: "reference", selector: emptySelector(), missingPolicy: { kind: "error" } };
  if (kind === "unary") return { kind: "unary", operator: "not", operand: literalNode() };
  if (kind === "binary") return { kind: "binary", operator: "eq", left: literalNode(), right: literalNode() };
  if (kind === "conditional") return { kind: "conditional", condition: literalNode(), thenValue: literalNode(), elseValue: literalNode() };
  if (kind === "call") return { kind: "call", function: "contains", arguments: [literalNode(), literalNode()] };
  if (kind === "array") return { kind: "array", items: [] };
  if (kind === "object") return { kind: "object", fields: {} };
  return literalNode();
}

const emptySelector = (): ValueSelector => ({ namespace: "inputs", run: { kind: "current" }, item: { kind: "current" }, path: [] });
const kindLabel = (kind: string) => ({ literal: "固定值", reference: "变量", unary: "一元运算", binary: "比较 / 算术", conditional: "条件", call: "函数", array: "数组", object: "对象" })[kind] ?? kind;
const binaryLabel = (operator: string) => ({ eq: "=", ne: "≠", gt: ">", gte: "≥", lt: "<", lte: "≤", add: "+", subtract: "−", multiply: "×", divide: "÷", modulo: "%", and: "AND", or: "OR", in: "IN" })[operator] ?? operator;
function parseLiteral(value: string): unknown { const trimmed = value.trim(); if (trimmed === "true") return true; if (trimmed === "false") return false; if (trimmed === "null") return null; if (trimmed !== "" && Number.isFinite(Number(trimmed))) return Number(trimmed); return value; }
function literalText(value: unknown) { return typeof value === "string" ? value : JSON.stringify(value) ?? ""; }
function parsePath(value: string): Array<string | number> { return value.split('.').map((segment) => segment.trim()).filter(Boolean).map((segment) => /^\d+$/.test(segment) ? Number(segment) : segment); }
function selectorLabel(selector: ValueSelector, catalog?: ReferenceCatalog) { return selectorDisplayLabel(selector, catalog) || "选择变量"; }
