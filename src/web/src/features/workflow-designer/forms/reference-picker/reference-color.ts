const NODE_TYPE_COLORS: Record<string, string> = {
  model: "#6366f1",
  agent: "#6366f1",
  http: "#8b5cf6",
  sub_workflow: "#8b5cf6",
  if: "#06b6d4",
  list: "#06b6d4",
  merge: "#06b6d4",
  loop: "#06b6d4",
  loop_over_items: "#06b6d4",
  approval: "#06b6d4",
};

export const DEFAULT_REFERENCE_COLOR = "#3b82f6";

export function sourceNodeColor(nodeType?: string): string {
  return (nodeType && NODE_TYPE_COLORS[nodeType]) || DEFAULT_REFERENCE_COLOR;
}
