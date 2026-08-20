import { LexicalComposer } from "@lexical/react/LexicalComposer";
import { ContentEditable } from "@lexical/react/LexicalContentEditable";
import { LexicalErrorBoundary } from "@lexical/react/LexicalErrorBoundary";
import { HistoryPlugin } from "@lexical/react/LexicalHistoryPlugin";
import { OnChangePlugin } from "@lexical/react/LexicalOnChangePlugin";
import { PlainTextPlugin } from "@lexical/react/LexicalPlainTextPlugin";
import { useLexicalComposerContext } from "@lexical/react/LexicalComposerContext";
import {
  $createParagraphNode,
  $createTextNode,
  $getRoot,
  $getSelection,
  $insertNodes,
  $isRangeSelection,
  $isTextNode,
  DecoratorNode,
  type EditorConfig,
  type ElementNode,
  type LexicalEditor,
  type LexicalNode,
  type NodeKey,
  type SerializedLexicalNode,
  type Spread,
} from "lexical";
import { Braces } from "lucide-react";
import { useEffect, useRef } from "react";

import type {
  DynamicValue,
  ReferenceCatalog,
  ReferenceEntry,
  TemplateSegment,
  ValueSelector,
} from "../model/types";

const AGENTX_DYNAMIC_MIME = "application/x-agentx-dynamic-value+json";

type SerializedVariableNode = Spread<
  { selector: ValueSelector; label?: string },
  SerializedLexicalNode
>;

class VariableNode extends DecoratorNode<React.ReactNode> {
  __selector: ValueSelector;
  __label?: string;

  static getType() {
    return "agentx-variable";
  }

  static clone(node: VariableNode) {
    return new VariableNode(structuredClone(node.__selector), node.__label, node.__key);
  }

  static importJSON(value: SerializedVariableNode) {
    return new VariableNode(value.selector, value.label);
  }

  constructor(selector: ValueSelector, label?: string, key?: NodeKey) {
    super(key);
    this.__selector = selector;
    this.__label = label;
  }

  createDOM(_config: EditorConfig) {
    const element = document.createElement("span");
    element.className = "inline-flex align-middle";
    return element;
  }

  updateDOM() {
    return false;
  }

  isInline() {
    return true;
  }

  isKeyboardSelectable() {
    return true;
  }

  exportJSON(): SerializedVariableNode {
    return {
      ...super.exportJSON(),
      selector: structuredClone(this.__selector),
      label: this.__label,
      type: "agentx-variable",
      version: 1,
    };
  }

  decorate() {
    return (
      <span
        className="mx-0.5 inline-flex h-6 max-w-56 select-none items-center gap-1 rounded-md border border-primary/25 bg-primary/10 px-1.5 align-middle text-xs font-medium text-primary"
        data-agentx-variable
        title={this.__label ?? fallbackSelectorLabel(this.__selector)}
      >
        <Braces className="size-3 shrink-0" />
        <span className="truncate">{this.__label ?? fallbackSelectorLabel(this.__selector)}</span>
      </span>
    );
  }
}

const $createVariableNode = (selector: ValueSelector, label?: string) =>
  new VariableNode(structuredClone(selector), label);
const $isVariableNode = (node: LexicalNode | null | undefined): node is VariableNode =>
  node instanceof VariableNode;

export function VariableTokenEditor({
  value,
  onChange,
  multiline = false,
  catalog,
  onEditorReady,
}: {
  value: DynamicValue;
  onChange: (value: DynamicValue) => void;
  multiline?: boolean;
  catalog?: ReferenceCatalog;
  onEditorReady?: (editor: LexicalEditor) => void;
}) {
  const current = useRef(value);
  current.current = value;
  return (
    <LexicalComposer
      initialConfig={{
        namespace: "agentx-variable-token-editor",
        nodes: [VariableNode],
        onError: (error) => {
          throw error;
        },
        editorState: () => replaceEditorValue(value, catalog),
        theme: { paragraph: "m-0" },
      }}
    >
      <div
        className={`relative rounded-md border border-border bg-surface text-xs focus-within:border-primary focus-within:ring-2 focus-within:ring-primary/15 ${multiline ? "min-h-24" : "min-h-9"}`}
        data-testid="variable-token-editor"
      >
        <PlainTextPlugin
          contentEditable={
            <ContentEditable
              aria-label="Value"
              className={`outline-none ${multiline ? "min-h-24 px-2 py-2" : "min-h-9 px-2 py-1.5 leading-6"}`}
            />
          }
          ErrorBoundary={LexicalErrorBoundary}
          placeholder={
            <span className="pointer-events-none absolute left-2 top-2 text-muted-foreground">
              选择变量或输入固定值
            </span>
          }
        />
        <HistoryPlugin />
        <OnChangePlugin
          ignoreSelectionChange
          onChange={(state) => {
            state.read(() => {
              const next = editorValue();
              if (JSON.stringify(next) !== JSON.stringify(current.current)) onChange(next);
            });
          }}
        />
        <EditorBridge catalog={catalog} onReady={onEditorReady} value={value} />
      </div>
    </LexicalComposer>
  );
}

export function insertVariable(
  editor: LexicalEditor,
  selector: ValueSelector,
  label?: string,
) {
  editor.update(() => {
    $insertNodes([$createVariableNode(selector, label)]);
  });
  editor.focus();
}

function EditorBridge({
  value,
  catalog,
  onReady,
}: {
  value: DynamicValue;
  catalog?: ReferenceCatalog;
  onReady?: (editor: LexicalEditor) => void;
}) {
  const [editor] = useLexicalComposerContext();
  const lastExternal = useRef(JSON.stringify(value));
  useEffect(() => onReady?.(editor), [editor, onReady]);
  useEffect(() => {
    const serialized = JSON.stringify(value);
    editor.getEditorState().read(() => {
      if (JSON.stringify(editorValue()) === serialized) {
        lastExternal.current = serialized;
        return;
      }
      if (lastExternal.current === serialized) return;
      editor.update(() => replaceEditorValue(value, catalog));
      lastExternal.current = serialized;
    });
  }, [catalog, editor, value]);
  useEffect(() => {
    const root = editor.getRootElement();
    if (!root) return;
    const copy = (event: ClipboardEvent) => {
      event.clipboardData?.setData(AGENTX_DYNAMIC_MIME, JSON.stringify(editorValueRead(editor)));
    };
    const paste = (event: ClipboardEvent) => {
      const encoded = event.clipboardData?.getData(AGENTX_DYNAMIC_MIME);
      if (!encoded) return;
      try {
        const dynamic = JSON.parse(encoded) as DynamicValue;
        event.preventDefault();
        editor.update(() => {
          const selection = $getSelection();
          if ($isRangeSelection(selection)) selection.removeText();
          $insertNodes(nodesForValue(dynamic));
        });
      } catch {
        // Ignore malformed external clipboard payloads and keep plain-text paste.
      }
    };
    root.addEventListener("copy", copy);
    root.addEventListener("cut", copy);
    root.addEventListener("paste", paste);
    return () => {
      root.removeEventListener("copy", copy);
      root.removeEventListener("cut", copy);
      root.removeEventListener("paste", paste);
    };
  }, [editor]);
  return null;
}

function editorValueRead(editor: LexicalEditor) {
  let value: DynamicValue = { kind: "literal", value: "" };
  editor.getEditorState().read(() => {
    value = editorValue();
  });
  return value;
}

function editorValue(): DynamicValue {
  const paragraph = $getRoot().getFirstChild();
  const segments: TemplateSegment[] = [];
  for (const node of (paragraph as ElementNode | null)?.getChildren() ?? []) {
    if ($isVariableNode(node)) {
      segments.push({
        kind: "reference",
        selector: structuredClone(node.__selector),
        missingPolicy: { kind: "error" },
      });
    } else if ($isTextNode(node) && node.getTextContent()) {
      const previous = segments.at(-1);
      if (previous?.kind === "text") previous.text += node.getTextContent();
      else segments.push({ kind: "text", text: node.getTextContent() });
    }
  }
  if (segments.length === 1 && segments[0].kind === "reference") {
    return {
      kind: "reference",
      selector: segments[0].selector,
      missingPolicy: segments[0].missingPolicy,
    };
  }
  if (segments.every((segment) => segment.kind === "text")) {
    return {
      kind: "literal",
      value: segments.map((segment) => segment.kind === "text" ? segment.text : "").join(""),
    };
  }
  return { kind: "template", segments };
}

function replaceEditorValue(value: DynamicValue, catalog?: ReferenceCatalog) {
  const root = $getRoot();
  root.clear();
  const paragraph = $createParagraphNode();
  paragraph.append(...nodesForValue(value, catalog));
  root.append(paragraph);
}

function nodesForValue(value: DynamicValue, catalog?: ReferenceCatalog): LexicalNode[] {
  if (value.kind === "literal") return [$createTextNode(String(value.value ?? ""))];
  if (value.kind === "reference") return [$createVariableNode(value.selector, selectorDisplayLabel(value.selector, catalog))];
  if (value.kind === "template") {
    return value.segments.map((segment) =>
      segment.kind === "text"
        ? $createTextNode(segment.text)
        : $createVariableNode(segment.selector, selectorDisplayLabel(segment.selector, catalog)),
    );
  }
  return [];
}

function fallbackSelectorLabel(selector: ValueSelector) {
  const source = selector.namespace === "outputs"
    ? selector.sourceNodeId?.slice(0, 8) ?? "output"
    : selector.namespace;
  return [source, selector.port, ...selector.path].filter(Boolean).join(" / ");
}

export function selectorDisplayLabel(selector: ValueSelector, catalog?: ReferenceCatalog) {
  const chain = catalog ? findEntryChain(catalog, selector) : undefined;
  if (!chain) return fallbackSelectorLabel(selector);
  return chain
    .map((entry) => entry.label)
    .filter((label) => !["current", "first", "last", "all()", "runs"].includes(label))
    .join(" / ");
}

function findEntryChain(catalog: ReferenceCatalog, selector: ValueSelector): ReferenceEntry[] | undefined {
  const visit = (entry: ReferenceEntry, parents: ReferenceEntry[]): ReferenceEntry[] | undefined => {
    const chain = [...parents, entry];
    if (entry.selector && JSON.stringify(entry.selector) === JSON.stringify(selector)) return chain;
    for (const child of entry.children) {
      const found = visit(child, chain);
      if (found) return found;
    }
    return undefined;
  };
  for (const entries of Object.values(catalog)) {
    for (const entry of entries ?? []) {
      const found = visit(entry, []);
      if (found) return found;
    }
  }
  return undefined;
}
