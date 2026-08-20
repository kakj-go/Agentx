import type {
  JsonSchemaProperty,
  NodeManifest,
  ReferenceCatalog,
  ReferenceEntry,
  StudioDocument,
  ValueSelector,
} from "../../model/types";

const EMPTY_CATALOG: ReferenceCatalog = {
  inputs: [],
  outputs: [],
  contexts: [],
};

export function buildReferenceCatalog(
  document: Pick<StudioDocument, "start" | "nodes" | "edges">,
  manifests: Map<string, NodeManifest>,
  targetNodeId?: string,
): ReferenceCatalog {
  const catalog = structuredClone(EMPTY_CATALOG);
  const inputSchema = asSchema(document.start.inputs);
  catalog.inputs = schemaEntries(
    inputSchema,
    "inputs",
    "inputs",
    false,
    baseSelector("inputs"),
  );
  catalog.contexts = Object.entries(document.start.contexts).map(
    ([name, definition]) => {
      const path = appendSegment("contexts", name);
      const schema = asSchema(definition.schema);
      return {
        id: path,
        label: name,
        path,
        selector: schema.properties
          ? undefined
          : baseSelector("contexts", [name]),
        type: schema.type,
        nullable: false,
        sensitive: definition.sensitive,
        scope: definition.scope,
        example: definition.default,
        children: schemaEntries(
          schema,
          path,
          path,
          definition.sensitive,
          baseSelector("contexts", [name]),
        ),
      };
    },
  );
  const predecessorIds = targetNodeId
    ? reachablePredecessors(document, targetNodeId)
    : new Set(document.nodes.map((node) => node.id));
  catalog.outputs = document.nodes.flatMap((node) => {
    const nodeData = node.data;
    if (nodeData.editorKind !== "action" || !predecessorIds.has(node.id)) return [];
    const manifest = manifests.get(
      `${nodeData.nodeType}@${nodeData.typeVersion}`,
    );
    if (!manifest) return [];
    const nodePath = appendSegment("outputs", nodeData.key);
    const projectedSchemaForPort = (portName: string) => {
      const outputSchema = asSchema(
        manifest.outputPortSchemas?.[portName] ?? manifest.outputSchema,
      );
      const projectedFields = nodeData.outputProjection[portName] ?? {};
      return Object.keys(projectedFields).length
        ? {
            ...outputSchema,
            properties: {
              ...(outputSchema.properties ?? {}),
              ...Object.fromEntries(
                Object.entries(projectedFields).map(([name, field]) => [
                  name,
                  field.schema ?? {},
                ]),
              ),
            },
          }
        : outputSchema;
    };
    const ports = manifest.outputPorts.map((port) => {
      const path = appendSegment(nodePath, port.name);
      const cardinality = manifest.outputCardinality?.[port.name] ?? "many";
      const projectedSchema = projectedSchemaForPort(port.name);
      const itemFields = schemaEntries(
        projectedSchema,
        `${path}.current.json`,
        `${path}.current.json`,
        false,
        outputSelector(node.id, port.name, { kind: "current" }),
      );
      const selectors: ReferenceEntry[] = [];
      if (manifest.expressionCapabilities?.supportsCurrent !== false)
        selectors.push(selector("current", path, itemFields, cardinality, node.id, port.name));
      if (manifest.expressionCapabilities?.supportsFirstLast !== false) {
        selectors.push(
          selector("first", path, itemFields, cardinality, node.id, port.name),
          selector("last", path, itemFields, cardinality, node.id, port.name),
        );
      }
      if (manifest.expressionCapabilities?.supportsAll !== false)
        selectors.push({
          id: `${path}.all()`,
          label: "all()",
          path: `${path}.all()`,
          selector: outputSelector(node.id, port.name, { kind: "all" }),
          type: "array",
          cardinality,
          nullable: false,
          children: [],
        });
      return {
        id: path,
        label: port.name,
        path,
        cardinality,
        nullable: cardinality === "zero_or_one" || cardinality === "zero_or_many",
        children: selectors,
      };
    });
    const runs =
      manifest.expressionCapabilities?.supportsRunSelection === false
        ? []
        : [
            {
              id: `${nodePath}.runs`,
              label: "runs",
              path: `${nodePath}.runs`,
              children: [
                {
                  id: `${nodePath}.runs["0"]`,
                  label: "run 0",
                  path: `${nodePath}.runs["0"]`,
                  children: manifest.outputPorts.map((port) => {
                    const path = `${nodePath}.runs["0"].${port.name}[0].json`;
                    const fields = schemaEntries(
                      projectedSchemaForPort(port.name),
                      path,
                      path,
                      false,
                      outputSelector(
                        node.id,
                        port.name,
                        { kind: "index", index: 0 },
                        { kind: "index", index: 0 },
                      ),
                    );
                    return {
                      id: path,
                      label: port.name,
                      path,
                      selector: fields.length
                        ? undefined
                        : outputSelector(
                            node.id,
                            port.name,
                            { kind: "index", index: 0 },
                            { kind: "index", index: 0 },
                          ),
                      type: "object",
                      cardinality:
                        manifest.outputCardinality?.[port.name] ?? "many",
                      nullable: true,
                      children: fields,
                    };
                  }),
                },
              ],
            } satisfies ReferenceEntry,
          ];
    return [
      {
        id: nodePath,
        label: nodeData.key,
        path: nodePath,
        children: [...ports, ...runs],
      },
    ];
  });
  return catalog;
}

function reachablePredecessors(
  document: Pick<StudioDocument, "nodes" | "edges">,
  targetNodeId: string,
) {
  const incoming = new Map<string, string[]>();
  for (const edge of document.edges)
    if (edge.data?.edgeKind === "execution")
      incoming.set(edge.target, [
        ...(incoming.get(edge.target) ?? []),
        edge.source,
      ]);
  const result = new Set<string>();
  const pending = [...(incoming.get(targetNodeId) ?? [])];
  while (pending.length) {
    const id = pending.pop()!;
    if (result.has(id)) continue;
    result.add(id);
    pending.push(...(incoming.get(id) ?? []));
  }
  return result;
}

function selector(
  name: "current" | "first" | "last",
  portPath: string,
  fields: ReferenceEntry[],
  cardinality: ReferenceEntry["cardinality"],
  sourceNodeId: string,
  port: string,
): ReferenceEntry {
  const path = `${portPath}.${name}`;
  return {
    id: path,
    label: name,
    path,
    selector: fields.length
      ? undefined
      : outputSelector(sourceNodeId, port, { kind: name }),
    type: "object",
    cardinality,
    nullable: cardinality === "zero_or_one" || cardinality === "zero_or_many",
    children: fields.map((field) =>
      rebase(field, `${portPath}.current`, path, { kind: name }),
    ),
  };
}

function rebase(
  entry: ReferenceEntry,
  from: string,
  to: string,
  item: ValueSelector["item"],
): ReferenceEntry {
  const path = entry.path.replace(from, to);
  return {
    ...entry,
    id: path,
    path,
    selector: entry.selector ? { ...entry.selector, item } : undefined,
    children: entry.children.map((child) => rebase(child, from, to, item)),
  };
}

function schemaEntries(
  schema: JsonSchemaProperty,
  parentPath: string,
  idPrefix: string,
  sensitive = false,
  parentSelector?: ValueSelector,
): ReferenceEntry[] {
  return Object.entries(schema.properties ?? {}).map(([name, child]) => {
    const path = appendSegment(parentPath, name);
    const id = appendSegment(idPrefix, name);
    const selector = parentSelector
      ? { ...parentSelector, path: [...parentSelector.path, name] }
      : undefined;
    const children = schemaEntries(child, path, id, sensitive, selector);
    return {
      id,
      label: name,
      path,
      selector: children.length ? undefined : selector,
      type: child.type,
      nullable: !(schema.required ?? []).includes(name),
      sensitive,
      children,
    };
  });
}

function asSchema(value: unknown): JsonSchemaProperty {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as JsonSchemaProperty)
    : {};
}

function baseSelector(
  namespace: ValueSelector["namespace"],
  path: Array<string | number> = [],
): ValueSelector {
  return {
    namespace,
    run: { kind: "current" },
    item: { kind: "current" },
    path,
  };
}

function outputSelector(
  sourceNodeId: string,
  port: string,
  item: ValueSelector["item"],
  run: ValueSelector["run"] = { kind: "current" },
): ValueSelector {
  return {
    namespace: "outputs",
    sourceNodeId,
    port,
    run,
    item,
    path: [],
  };
}

const CEL_RESERVED = new Set([
  "as",
  "break",
  "const",
  "continue",
  "else",
  "false",
  "for",
  "function",
  "if",
  "import",
  "in",
  "let",
  "loop",
  "namespace",
  "null",
  "package",
  "return",
  "true",
  "var",
  "void",
  "while",
]);

function appendSegment(parent: string, segment: string) {
  return /^[A-Za-z_][A-Za-z0-9_]*$/.test(segment) && !CEL_RESERVED.has(segment)
    ? `${parent}.${segment}`
    : `${parent}[${JSON.stringify(segment)}]`;
}
