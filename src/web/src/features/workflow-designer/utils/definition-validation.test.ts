import { describe, expect, it } from "vitest";

import type { StudioDocument } from "../model/types";
import { definitionIssues, isInputBindingEmpty, isReferenceKey } from "./definition-validation";

const document = (): StudioDocument => ({
  start: { inputs: { type: "object" }, contexts: {} },
  nodes: [],
  edges: [],
  end: { completion: "first_return", outputs: {}, error: { outputs: { } } },
  boundaryLayouts: [],
  viewport: { x: 0, y: 0, zoom: 1 },
  annotations: [],
  groups: [],
  settings: { executionOrder: "deterministic", activationBudget: 10_000 },
});

describe("workflow definition validation", () => {
  it("matches the backend reference-key contract", () => {
    expect(isReferenceKey("answer_2")).toBe(true);
    expect(isReferenceKey("_answer")).toBe(true);
    expect(isReferenceKey("2answer")).toBe(false);
    expect(isReferenceKey("Answer")).toBe(false);
    expect(isReferenceKey("中文")).toBe(false);
  });

  it("distinguishes missing End output content from valid falsey values and references", () => {
    expect(isInputBindingEmpty({ kind: "literal", value: "  " })).toBe(true);
    expect(isInputBindingEmpty({ kind: "literal", value: 0 })).toBe(false);
    expect(isInputBindingEmpty({ kind: "literal", value: false })).toBe(false);
    expect(isInputBindingEmpty({ kind: "reference", selector: { namespace: "inputs", run: { kind: "current" }, item: { kind: "current" }, path: ["question"] }, missingPolicy: { kind: "error" } })).toBe(false);
  });

  it("finds user-editable values rejected by backend definition validation", () => {
    const value = document();
    value.settings.activationBudget = 0;
    value.start.contexts["Bad-name"] = { schema: { type: "string" }, default: "", mutable: true, sensitive: false, clientWritable: false, scope: "execution_tree", maxSize: 0, mergePolicy: "replace" };
    value.end.outputs["中文"] = { schema: { type: "string" }, required: false, sensitive: false };
    value.end.outputs["answer"] = { schema: { type: "string" }, required: false, sensitive: false };
    value.nodes = [{ id: "exit", type: "exit", position: { x: 0, y: 0 }, data: { editorKind: "exit", key: "exit", label: "End", protected: false, parameters: { outputs: { answer: { kind: "literal", value: "  " } }, errorOutputs: {} } } }];

    expect(definitionIssues(value).map((issue) => issue.code)).toEqual(expect.arrayContaining([
      "INVALID_ACTIVATION_BUDGET",
      "INVALID_CONTEXT_KEY",
      "INVALID_CONTEXT_MAX_SIZE",
      "INVALID_END_OUTPUT_KEY",
      "END_OUTPUT_BINDING_REQUIRED",
    ]));
  });

  it("keeps end node mappings aligned with the shared contract", () => {
    const value = document();
    value.end.outputs["answer"] = { schema: { type: "string" }, required: true, sensitive: false };
    value.nodes = [{ id: "exit", type: "exit", position: { x: 0, y: 0 }, data: { editorKind: "exit", key: "exit", label: "End", protected: false, parameters: { outputs: { ghost: { kind: "literal", value: "x" } }, errorOutputs: {} } } }];

    expect(definitionIssues(value).map((issue) => issue.code)).toEqual(expect.arrayContaining([
      "EXIT_MAPPING_KEY_UNKNOWN",
      "EXIT_REQUIRED_MAPPING_MISSING",
    ]));
  });

  it("rejects a document without any end node", () => {
    expect(definitionIssues(document()).map((issue) => issue.code)).toContain("EXIT_REQUIRED");
  });

  it("checks generated node and connection invariants before sending a draft", () => {
    const value = document();
    value.start.inputs = [] as never;
    value.nodes = [{
      id: "__end__",
      type: "manifest",
      position: { x: 0, y: 0 },
      data: {
        editorKind: "action",
        nodeType: "Bad Type",
        typeVersion: 0,
        label: " ",
        key: "Bad-key",
        parameters: {},
        contextWrites: [],
        resourceReferences: [],
        settings: { maxTries: 0 },
        disabled: false,
      },
    }];
    value.edges = [{ id: "", source: "__end__", sourceHandle: "main", target: "missing", targetHandle: "main", data: { edgeKind: "execution", order: 0 } }];

    expect(definitionIssues(value).map((issue) => issue.code)).toEqual(expect.arrayContaining([
      "INVALID_INPUT_SCHEMA",
      "RESERVED_NODE_ID",
      "INVALID_NODE_KEY",
      "INVALID_NODE_TYPE",
      "INVALID_NODE_NAME",
      "UNSUPPORTED_NODE_VERSION",
      "INVALID_MAX_TRIES",
      "INVALID_CONNECTION_ID",
      "END_BOUNDARY_REMOVED",
    ]));
  });
});
