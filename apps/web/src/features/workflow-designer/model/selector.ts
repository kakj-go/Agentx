import type { ValueSelection, ValueSelector } from "./types";

const selectionEqual = (left: ValueSelection, right: ValueSelection) =>
  left.kind === right.kind && (left.kind !== "index" || right.kind === "index" && left.index === right.index);

export function selectorsEqual(left: ValueSelector, right: ValueSelector) {
  return left.namespace === right.namespace
    && left.sourceNodeId === right.sourceNodeId
    && left.port === right.port
    && selectionEqual(left.run, right.run)
    && selectionEqual(left.item, right.item)
    && left.path.length === right.path.length
    && left.path.every((segment, index) => segment === right.path[index]);
}
