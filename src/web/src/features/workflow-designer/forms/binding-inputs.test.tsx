import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";

import type { ReferenceCatalog } from "../model/types";
import { inputBindingFromValue, parseLiteral, ReferenceInput, SmartInput, TemplateInput } from "./binding-inputs";

const catalog: ReferenceCatalog = {
  inputs: [{
    id: "inputs.items",
    label: "items",
    path: "inputs.items",
    type: "array",
    schema: { type: "array", items: { type: "object", properties: { id: { type: "integer" } }, required: ["id"] } },
    selector: { namespace: "inputs", run: { kind: "current" }, item: { kind: "current" }, path: ["items"] },
    children: [],
  }, {
    id: "inputs.question",
    label: "question",
    path: "inputs.question",
    type: "string",
    schema: { type: "string" },
    selector: { namespace: "inputs", run: { kind: "current" }, item: { kind: "current" }, path: ["question"] },
    children: [],
  }],
  outputs: [],
  contexts: [],
};

describe("Dify-style binding inputs", () => {
  it("opens the variable picker directly without an expression mode", () => {
    const onChange = vi.fn();
    render(<ReferenceInput catalog={catalog} expectedSchema={{ type: "array", items: { type: "object" } }} onChange={onChange} />);

    fireEvent.click(screen.getByTestId("reference-input").getElementsByTagName("button")[0]);
    expect(screen.getByTestId("reference-picker")).toBeInTheDocument();
    expect(screen.queryByRole("combobox", { name: "Expression type" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /Inputs|输入/ }));
    fireEvent.click(screen.getByRole("button", { name: /items/ }));

    expect(onChange).toHaveBeenCalledWith(expect.objectContaining({
      kind: "reference",
      selector: expect.objectContaining({ namespace: "inputs", path: ["items"] }),
    }));
  });

  it("infers JSON literal types while preserving ambiguous numeric text", () => {
    expect(inputBindingFromValue(parseLiteral("001"))).toEqual({ kind: "literal", value: "001" });
    expect(inputBindingFromValue(parseLiteral("7"))).toEqual({ kind: "literal", value: 7 });
    expect(inputBindingFromValue(JSON.parse('{"approved":true}'))).toEqual({ kind: "object", fields: { approved: { kind: "literal", value: true } } });
  });

  it("opens the picker when an empty template click lands on its inner paragraph", () => {
    render(<TemplateInput catalog={catalog} onChange={vi.fn()} value={{ kind: "template", segments: [] }} />);
    const editor = screen.getByRole("textbox", { name: "Value" });
    fireEvent.click(editor.querySelector("p") ?? editor);
    expect(screen.getByTestId("reference-picker")).toBeInTheDocument();
  });

  it("inserts a variable after the picker takes focus away from the editor", async () => {
    const onChange = vi.fn();
    function Harness() {
      const [value, setValue] = useState<Parameters<typeof TemplateInput>[0]["value"]>({ kind: "literal", value: "" });
      return <TemplateInput catalog={catalog} onChange={(next) => { onChange(next); setValue(next); }} value={value} />;
    }
    render(<Harness />);
    fireEvent.click(screen.getByRole("textbox", { name: "Value" }));
    fireEvent.click(screen.getByRole("button", { name: /Inputs|输入/ }));
    fireEvent.click(screen.getByRole("button", { name: /question/ }));
    expect(onChange).toHaveBeenCalledWith(expect.objectContaining({ kind: "reference" }));
    await waitFor(() => expect(document.querySelector("[data-agentx-variable]")).toBeInTheDocument());
  });

  it("renders a persisted literal binding as its text instead of an object string", () => {
    render(<TemplateInput onChange={vi.fn()} value={{ kind: "literal", value: "Return the requested JSON object." }} />);
    expect(screen.getByRole("textbox", { name: "Value" })).toHaveTextContent("Return the requested JSON object.");
    expect(screen.getByRole("textbox", { name: "Value" })).not.toHaveTextContent("[object Object]");
  });

  it("uses strict JSON parsing with plain text fallback", () => {
    expect(parseLiteral("true")).toBe(true);
    expect(parseLiteral("[1,2]")).toEqual([1, 2]);
    expect(parseLiteral("001")).toBe("001");
  });

  it("opens the picker directly for an empty structured array input", () => {
    render(<SmartInput catalog={catalog} expectedSchema={{ type: "array", items: {} }} onChange={vi.fn()} value={{ kind: "literal", value: undefined }} />);
    fireEvent.click(screen.getByRole("textbox", { name: "Value" }));
    expect(screen.getByTestId("reference-picker")).toBeVisible();
  });

  it("synchronizes controlled literal updates without emitting duplicate changes", async () => {
    const onChange = vi.fn();
    const rendered = render(<SmartInput onChange={onChange} value={{ kind: "literal", value: 1 }} />);
    rendered.rerender(<SmartInput onChange={onChange} value={{ kind: "literal", value: 12 }} />);

    await waitFor(() => expect(screen.getByRole("textbox", { name: "Value" })).toHaveTextContent("12"));
    expect(onChange).not.toHaveBeenCalled();
  });
});
