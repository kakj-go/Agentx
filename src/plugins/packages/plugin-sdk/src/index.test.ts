import { describe, expect, it } from "vitest";

import {
  PLUGIN_PROTOCOL_VERSION,
  PLUGIN_SDK_API_VERSION,
  completed,
  defineExecute,
  failed,
} from "./index.js";

describe("plugin SDK contract", () => {
  it("pins the public protocol and SDK API to version 1", () => {
    expect(PLUGIN_PROTOCOL_VERSION).toBe(1);
    expect(PLUGIN_SDK_API_VERSION).toBe(2);
  });

  it("constructs stable completed and failed results", () => {
    expect(completed({ main: [{ json: { value: 42 } }] })).toEqual({
      status: "completed",
      outputs: { main: [{ json: { value: 42 } }] },
    });
    expect(failed("INVALID_INPUT", "bad value", true, { field: "value" })).toEqual({
      status: "failed",
      code: "INVALID_INPUT",
      message: "bad value",
      retryable: true,
      details: { field: "value" },
    });
  });

  it("preserves the execute function identity", () => {
    const execute = defineExecute(async () => completed({ main: [] }));
    expect(execute).toBeTypeOf("function");
  });
});
