import { describe, expect, it } from "vitest";

import { selectorsEqual } from "./selector";
import type { ValueSelector } from "./types";

describe("selectorsEqual", () => {
  it("ignores JSON property insertion order", () => {
    const left = JSON.parse('{"namespace":"execution","run":{"kind":"current"},"item":{"kind":"current"},"path":["workflow","name"]}') as ValueSelector;
    const right = JSON.parse('{"path":["workflow","name"],"item":{"kind":"current"},"run":{"kind":"current"},"namespace":"execution"}') as ValueSelector;
    expect(selectorsEqual(left, right)).toBe(true);
  });

  it("keeps indexed selections and paths distinct", () => {
    const left = { namespace: "outputs", sourceNodeId: "node", port: "main", run: { kind: "current" }, item: { kind: "index", index: 1 }, path: ["value"] } as ValueSelector;
    expect(selectorsEqual(left, { ...left, item: { kind: "index", index: 2 } })).toBe(false);
    expect(selectorsEqual(left, { ...left, path: ["other"] })).toBe(false);
  });
});
