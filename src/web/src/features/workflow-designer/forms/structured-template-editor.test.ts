import { describe, expect, it } from "vitest";

import type { InputBinding, InputTemplateSegment, ValueSelector } from "../model/types";
import { bindingToEditorValue, parseStructuredEditorValue } from "./structured-template-editor";

const selector: ValueSelector = {
  namespace: "inputs",
  run: { kind: "current" },
  item: { kind: "current" },
  path: ["value"],
};
const reference: Extract<InputTemplateSegment, { kind: "reference" }> = {
  kind: "reference",
  selector,
  missingPolicy: { kind: "error" },
};

describe("StructuredTemplateEditor JSON5 contract", () => {
  it("parses standalone variables as typed values and string variables as templates", () => {
    const value = parseStructuredEditorValue({
      kind: "template",
      segments: [
        { kind: "text", text: "['a', " },
        reference,
        { kind: "text", text: ", { label: 'prefix " },
        reference,
        { kind: "text", text: " suffix', },]" },
      ],
    });
    expect(value).toEqual({
      kind: "array",
      items: [
        { kind: "literal", value: "a" },
        { kind: "reference", selector, missingPolicy: { kind: "error" } },
        { kind: "object", fields: { label: { kind: "template", segments: [{ kind: "text", text: "prefix " }, reference, { kind: "text", text: " suffix" }] } } },
      ],
    });
  });

  it("round-trips recursive bindings without exposing selector ids as text", () => {
    const input: InputBinding = { kind: "object", fields: { rows: { kind: "array", items: [{ kind: "reference", selector, missingPolicy: { kind: "null" } }] } } };
    expect(parseStructuredEditorValue(bindingToEditorValue(input))).toEqual(input);
  });
});
