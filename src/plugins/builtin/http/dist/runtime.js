export const execute = async (context) => {
    const request = { ...context.parameters, body: context.parameters.body ?? context.inputs.main?.[0]?.json ?? null, idempotencyKey: context.execution.idempotencyKey };
    const response = await context.http(request);
    return { status: 'completed', outputs: { main: [{ json: response }] } };
};
