export const resolveDefinition = (configuration, upstreamContracts, context) => {
    if (context.nodeType === 'set') {
        const incoming = schemaObject(upstreamContracts.main)?.schema ?? {};
        const contracts = { ...upstreamContracts, $currentItem: incoming };
        const schema = configuration.keepOnlySet === true
            ? { type: 'object', properties: {}, required: [], additionalProperties: false }
            : cloneSchema(incoming);
        const properties = schemaObject(schema.properties) ?? {};
        const required = new Set(Array.isArray(schema.required) ? schema.required.filter((value) => typeof value === 'string') : []);
        const values = bindingFields(configuration.values);
        for (const [name, binding] of Object.entries(values)) {
            properties[name] = bindingSchema(binding, contracts);
            required.add(name);
        }
        schema.type = 'object';
        schema.properties = properties;
        schema.required = [...required];
        return { status: 'complete', outputSchema: schema, outputPortSchemas: { main: schema } };
    }
    if (context.nodeType === 'list') {
        const input = bindingSchema(configuration.input, upstreamContracts);
        const item = schemaObject(input)?.items ?? {};
        const issues = listConditionIssues(configuration, { ...upstreamContracts, $currentItem: item });
        const schema = { type: 'object', properties: { items: { type: 'array', items: item } }, required: ['items'], additionalProperties: false };
        return { status: issues.length ? 'invalid' : 'complete', outputSchema: schema, outputPortSchemas: { main: schema }, issues };
    }
    return { status: 'invalid', issues: [{ path: '', code: 'BUILTIN_NODE_UNSUPPORTED', message: `Unsupported data node ${context.nodeType}` }] };
};
export const execute = (context) => {
    if (context.execution.nodeType === 'set')
        return executeSet(context.inputs, context.parameters, context.perItemParameters);
    if (context.execution.nodeType === 'list')
        return executeList(context.parameters, context.perItemParameters);
    return { status: 'failed', code: 'BUILTIN_NODE_UNSUPPORTED', message: `Unsupported data node ${context.execution.nodeType}` };
};
function executeSet(inputs, common, perItem) {
    const main = inputs.main ?? [];
    const offset = Object.entries(inputs).filter(([name]) => name < 'main').reduce((total, [, items]) => total + items.length, 0);
    const output = main.map((item, index) => {
        const parameters = perItem[offset + index] ?? common;
        const values = object(parameters.values);
        const base = parameters.keepOnlySet === true ? {} : object(item.json);
        return { ...item, json: { ...base, ...values } };
    });
    return { status: 'completed', outputs: { main: output } };
}
function executeList(common, perItem) {
    if (!Array.isArray(common.input))
        return { status: 'failed', code: 'LIST_INPUT_NOT_ARRAY', message: 'List input must resolve to an array' };
    const kept = common.input.map((json, index) => ({ index, item: { json } })).filter(({ index }) => passes(perItem[index] ?? common));
    const fields = Array.isArray(common.sort) ? common.sort : [];
    if (fields.length)
        kept.sort((left, right) => compareFields(perItem[left.index] ?? common, perItem[right.index] ?? common, fields));
    const take = typeof common.takeN === 'number' && common.takeN >= 0 ? common.takeN : kept.length;
    return { status: 'completed', outputs: { main: [{ json: { items: kept.slice(0, take).map(({ item }) => item.json) } }] } };
}
function passes(parameters) {
    const filter = object(parameters.filter);
    const conditions = Array.isArray(filter.conditions) ? filter.conditions.map((entry) => object(entry).condition === true) : [];
    return filter.logicalOp === 'or' ? conditions.some(Boolean) : conditions.every(Boolean);
}
function compareFields(left, right, fields) {
    const leftFields = Array.isArray(left.sort) ? left.sort : [];
    const rightFields = Array.isArray(right.sort) ? right.sort : [];
    for (let index = 0; index < fields.length; index += 1) {
        const specification = object(fields[index]);
        const ordering = compareJson(object(leftFields[index]).selector, object(rightFields[index]).selector, specification.nulls === 'first');
        if (ordering !== 0)
            return specification.direction === 'desc' ? -ordering : ordering;
    }
    return 0;
}
function compareJson(left, right, nullsFirst) {
    const leftMissing = left === null || left === undefined;
    const rightMissing = right === null || right === undefined;
    if (leftMissing && rightMissing)
        return 0;
    if (leftMissing)
        return nullsFirst ? -1 : 1;
    if (rightMissing)
        return nullsFirst ? 1 : -1;
    if (typeof left === 'number' && typeof right === 'number')
        return left === right ? 0 : left < right ? -1 : 1;
    return canonical(left).localeCompare(canonical(right));
}
function canonical(value) {
    if (Array.isArray(value))
        return `[${value.map(canonical).join(',')}]`;
    if (value && typeof value === 'object')
        return `{${Object.keys(value).sort().map((key) => `${JSON.stringify(key)}:${canonical(value[key])}`).join(',')}}`;
    return JSON.stringify(value);
}
function object(value) {
    return value && typeof value === 'object' && !Array.isArray(value) ? value : {};
}
function schemaObject(value) {
    return value && typeof value === 'object' && !Array.isArray(value) ? value : undefined;
}
function cloneSchema(value) {
    const cloned = JSON.parse(JSON.stringify(value));
    return schemaObject(cloned) ?? { type: 'object' };
}
function bindingFields(value) {
    const binding = object(value);
    return binding.kind === 'object' ? object(binding.fields) : {};
}
function bindingSchema(value, upstream) {
    const binding = object(value);
    switch (binding.kind) {
        case 'literal': return literalSchema(binding.value);
        case 'template': return { type: 'string' };
        case 'array': {
            const items = Array.isArray(binding.items) ? binding.items.map((item) => bindingSchema(item, upstream)) : [];
            const first = items[0];
            return { type: 'array', items: first && items.every((item) => JSON.stringify(item) === JSON.stringify(first)) ? first : {} };
        }
        case 'object': {
            const fields = object(binding.fields);
            return { type: 'object', properties: Object.fromEntries(Object.entries(fields).map(([name, child]) => [name, bindingSchema(child, upstream)])), required: Object.keys(fields) };
        }
        case 'reference': {
            const selector = object(binding.selector);
            const path = Array.isArray(selector.path) ? selector.path : [];
            if (selector.namespace === 'inputs')
                return schemaAt(upstream.$inputs, path);
            if (selector.namespace === 'contexts' && typeof path[0] === 'string') {
                const contexts = schemaObject(upstream.$contexts);
                const context = schemaObject(contexts?.[path[0]]);
                return schemaAt(context?.schema, path.slice(1));
            }
            if (selector.namespace === 'outputs' && typeof selector.sourceNodeId === 'string') {
                const nodes = schemaObject(upstream.$nodes);
                const node = schemaObject(nodes?.[selector.sourceNodeId]);
                const ports = schemaObject(node?.ports);
                const port = typeof selector.port === 'string' ? selector.port : 'main';
                return schemaAt(ports?.[port], path);
            }
            if (selector.namespace === 'item') {
                return schemaAt(upstream.$currentItem, path);
            }
            return {};
        }
        default: return {};
    }
}
function listConditionIssues(configuration, upstream) {
    const filter = object(configuration.filter);
    const rows = Array.isArray(filter.conditions) ? filter.conditions : [];
    return rows.flatMap((entry, index) => {
        const condition = object(object(entry).condition);
        const left = bindingSchema(condition.left, upstream);
        const right = bindingSchema(condition.right, upstream);
        const operator = typeof condition.operator === 'string' ? condition.operator : '';
        if (schemasCompatible(left, right, operator))
            return [];
        return [{ path: `filter.conditions[${index}].condition.right`, code: 'CONDITION_OPERAND_TYPE_MISMATCH', message: 'Condition operands have incompatible schemas' }];
    });
}
function schemasCompatible(left, right, operator) {
    const leftTypes = schemaTypes(left);
    const rightTypes = schemaTypes(right);
    if (!leftTypes.length || !rightTypes.length)
        return true;
    if (['gt', 'gte', 'lt', 'lte'].includes(operator)) {
        return (leftTypes.some((value) => value === 'number' || value === 'integer') && rightTypes.some((value) => value === 'number' || value === 'integer'))
            || (leftTypes.includes('string') && rightTypes.includes('string'));
    }
    if (['contains', 'not_contains', 'starts_with', 'ends_with', 'matches'].includes(operator))
        return leftTypes.includes('string') || leftTypes.includes('array');
    return leftTypes.some((value) => rightTypes.includes(value) || (value === 'integer' && rightTypes.includes('number')) || (value === 'number' && rightTypes.includes('integer')));
}
function schemaTypes(value) {
    const type = schemaObject(value)?.type;
    return Array.isArray(type) ? type.filter((item) => typeof item === 'string') : typeof type === 'string' ? [type] : [];
}
function schemaAt(value, path) {
    let current = value ?? {};
    for (const segment of path) {
        const schema = schemaObject(current);
        if (!schema)
            return {};
        current = typeof segment === 'number'
            ? schemaObject(schema.items) ?? {}
            : schemaObject(schema.properties)?.[String(segment)] ?? {};
    }
    return current;
}
function literalSchema(value) {
    if (value === null || value === undefined)
        return { type: 'null' };
    if (Array.isArray(value)) {
        const items = value.map(literalSchema);
        const first = items[0];
        return { type: 'array', items: first && items.every((item) => JSON.stringify(item) === JSON.stringify(first)) ? first : {} };
    }
    if (typeof value === 'object')
        return { type: 'object', properties: Object.fromEntries(Object.entries(value).map(([name, child]) => [name, literalSchema(child)])), required: Object.keys(value) };
    return { type: typeof value === 'number' ? (Number.isInteger(value) ? 'integer' : 'number') : typeof value };
}
