import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { once } from 'node:events'
import { createInterface } from 'node:readline'
import test from 'node:test'
import { fileURLToPath } from 'node:url'

const runner = fileURLToPath(new URL('./runner.mjs', import.meta.url))

function start() {
  const child = spawn(process.execPath, [runner], { stdio: ['pipe', 'pipe', 'pipe'] })
  const messages = []
  const waiters = []
  createInterface({ input: child.stdout, crlfDelay: Infinity }).on('line', (line) => {
    const message = JSON.parse(line)
    messages.push(message)
    for (const waiter of waiters.splice(0)) waiter()
  })
  const send = (message) => child.stdin.write(`${JSON.stringify({ jsonrpc: '2.0', ...message })}\n`)
  const writeRaw = (value) => child.stdin.write(value)
  const waitFor = async (predicate) => {
    while (true) {
      const found = messages.find(predicate)
      if (found) return found
      await new Promise((resolve) => waiters.push(resolve))
    }
  }
  return { child, messages, send, writeRaw, waitFor }
}

async function initialize(rpc) {
  rpc.send({ id: 'init', method: 'runner.initialize', params: { protocolVersion: 2, sdkApiVersion: 2 } })
  const initialized = await rpc.waitFor((message) => message.id === 'init')
  assert.equal(initialized.result.protocolVersion, 2)
}

test('executes an immutable module and streams trace events', async () => {
  const rpc = start()
  await initialize(rpc)
  rpc.send({
    id: 'attempt-1', method: 'node.execute', params: {
      invocationId: 'attempt-1', runtimeSource: 'export async function execute(ctx){return ctx.trace.span("work",span=>{span.content({type:"acme/test",version:1,data:{ok:true}});return {status:"completed",outputs:{main:[{json:{answer:ctx.parameters.answer}}]}}})}',
      inputs: { main: [] }, parameters: { answer: 42 }, execution: { deadline: new Date(Date.now() + 5_000).toISOString() },
    },
  })
  const response = await rpc.waitFor((message) => message.id === 'attempt-1')
  assert.deepEqual(response.result.outputs.main, [{ json: { answer: 42 } }])
  assert.equal(response.result.trace[0].contents[0].type, 'acme/test')
  assert.deepEqual(rpc.messages.filter((message) => message.method === 'trace.event').map((message) => message.params.phase), ['started', 'content', 'finished'])
  rpc.child.stdin.end()
  await once(rpc.child, 'close')
})

test('performs a bidirectional idempotent host call', async () => {
  const rpc = start()
  await initialize(rpc)
  rpc.send({
    id: 'attempt-host', method: 'node.execute', params: {
      invocationId: 'attempt-host', runtimeSource: 'export async function execute(ctx){const value=await ctx.trace.span("host parent",()=>ctx.http({method:"POST",url:"https://example.test",body:{ok:true},idempotencyKey:"write-1"}));return {status:"completed",outputs:{main:[{json:value}]}}}',
      execution: { deadline: new Date(Date.now() + 5_000).toISOString() },
    },
  })
  const host = await rpc.waitFor((message) => message.method === 'host.http')
  assert.equal(host.params.input.idempotencyKey, 'write-1')
  assert.equal(host.params.parentIndex, 0)
  rpc.send({ id: host.id, result: { status: 200, body: { accepted: true } } })
  const response = await rpc.waitFor((message) => message.id === 'attempt-host')
  assert.deepEqual(response.result.outputs.main[0].json, { status: 200, body: { accepted: true } })
  rpc.child.stdin.end()
  await once(rpc.child, 'close')
})

test('preserves an unknown host-call outcome for Runtime fencing', async () => {
  const rpc = start()
  await initialize(rpc)
  rpc.send({ id: 'unknown', method: 'node.execute', params: { invocationId: 'unknown', runtimeSource: 'export async function execute(ctx){await ctx.http({method:"POST",url:"https://example.test",idempotencyKey:"write-unknown"})}', execution: { deadline: new Date(Date.now() + 5_000).toISOString() } } })
  const host = await rpc.waitFor((message) => message.method === 'host.http')
  rpc.send({ id: host.id, error: { code: -32021, message: 'PROVIDER_OUTCOME_UNKNOWN: timed out after sending' } })
  const response = await rpc.waitFor((message) => message.id === 'unknown')
  assert.equal(response.error.code, -32021)
  rpc.child.stdin.end()
  await once(rpc.child, 'close')
})

test('rejects calls before handshake and unsupported APIs', async () => {
  const rpc = start()
  rpc.send({ id: 'early', method: 'node.resolveDefinition', params: {} })
  assert.equal((await rpc.waitFor((message) => message.id === 'early')).error.code, -32011)
  rpc.send({ id: 'bad-init', method: 'runner.initialize', params: { protocolVersion: 1, sdkApiVersion: 1 } })
  assert.equal((await rpc.waitFor((message) => message.id === 'bad-init')).error.code, -32010)
  rpc.child.stdin.end()
  await once(rpc.child, 'close')
})

test('cancels an asynchronous invocation', async (t) => {
  const rpc = start()
  t.after(() => rpc.child.kill())
  await initialize(rpc)
  rpc.send({ id: 'slow', method: 'node.execute', params: { invocationId: 'slow', runtimeSource: 'export async function execute(ctx){ctx.signal.throwIfAborted();await new Promise((resolve,reject)=>{const timer=setTimeout(resolve,10000);ctx.signal.addEventListener("abort",()=>{clearTimeout(timer);reject(new Error("cancelled"))},{once:true})});return {status:"completed",outputs:{main:[]}}}', execution: { deadline: new Date(Date.now() + 20_000).toISOString() } } })
  rpc.send({ id: 'cancel', method: 'invocation.cancel', params: { invocationId: 'slow' } })
  assert.equal((await rpc.waitFor((message) => message.id === 'cancel')).result.cancelled, true)
  assert.equal((await rpc.waitFor((message) => message.id === 'slow')).error.code, -32002)
  rpc.child.stdin.end()
  await once(rpc.child, 'close')
})

test('isolates module state and clears plugin background timers between invocations', async () => {
  const source = 'let calls=0;export async function execute(){calls+=1;setInterval(()=>{},10000);return {status:"completed",outputs:{main:[{json:{calls}}]}}}'
  for (const id of ['isolated-1', 'isolated-2']) {
    const rpc = start()
    await initialize(rpc)
    rpc.send({ id, method: 'node.execute', params: { invocationId: id, runtimeSource: source, execution: { deadline: new Date(Date.now() + 5_000).toISOString() } } })
    const response = await rpc.waitFor((message) => message.id === id)
    assert.equal(response.result.outputs.main[0].json.calls, 1)
    rpc.child.stdin.end()
    await once(rpc.child, 'close')
  }
})

test('keeps nested and concurrent Promise spans under their parent', async () => {
  const rpc = start()
  await initialize(rpc)
  rpc.send({
    id: 'nested', method: 'node.execute', params: {
      invocationId: 'nested',
      runtimeSource: 'export async function execute(ctx){await ctx.trace.span("outer",()=>Promise.all([ctx.trace.span("left",async()=>{}),ctx.trace.span("right",async()=>{})]));return {status:"completed",outputs:{main:[]}}}',
      execution: { deadline: new Date(Date.now() + 5_000).toISOString() },
    },
  })
  const response = await rpc.waitFor((message) => message.id === 'nested')
  assert.deepEqual(response.result.trace.map((span) => span.parentIndex), [undefined, 0, 0])
  rpc.child.stdin.end()
  await once(rpc.child, 'close')
})

test('drops excess trace content while preserving the business result', async () => {
  const rpc = start()
  await initialize(rpc)
  rpc.send({
    id: 'budget', method: 'node.execute', params: {
      invocationId: 'budget',
      runtimeSource: 'export async function execute(ctx){return ctx.trace.span("many",span=>{for(let i=0;i<33;i++)span.content({type:"acme/test",version:1,data:{i}});return {status:"completed",outputs:{main:[]}}})}',
      execution: { deadline: new Date(Date.now() + 5_000).toISOString() },
    },
  })
  const response = await rpc.waitFor((message) => message.id === 'budget')
  assert.equal(response.result.status, "completed")
  assert.equal(response.result.traceDiagnostics.dropped, 1)
  rpc.child.stdin.end()
  await once(rpc.child, 'close')
})

test('repeated module allocations stay within the invocation memory budget', async () => {
  const started = performance.now()
  for (let index = 0; index < 120; index += 1) {
    const rpc = start()
    await initialize(rpc)
    const id = `memory-${index}`
    rpc.send({ id, method: 'node.execute', params: { invocationId: id, runtimeSource: 'const lookup=new Array(131072).fill("value"); export async function execute(){return {status:"completed",outputs:{main:[{json:{size:lookup.length,heap:process.memoryUsage().heapUsed}}]}}}', execution: { deadline: new Date(Date.now() + 5_000).toISOString() } } })
    const response = await rpc.waitFor(message => message.id === id)
    assert.equal(response.result.outputs.main[0].json.size, 131072)
    assert.ok(response.result.outputs.main[0].json.heap < 32 * 1024 * 1024)
    rpc.child.stdin.end()
    await once(rpc.child, 'close')
  }
  assert.ok(performance.now() - started < 30_000)
})

test('refuses a second invocation in the same module environment', async () => {
  const rpc = start()
  await initialize(rpc)
  const params = { invocationId: 'first', runtimeSource: 'export async function execute(){return {status:"completed",outputs:{main:[]}}}', execution: {deadline:new Date(Date.now()+5000).toISOString()} }
  rpc.send({id:'first',method:'node.execute',params})
  await rpc.waitFor(message=>message.id==='first')
  rpc.send({id:'second',method:'node.execute',params:{...params,invocationId:'second'}})
  assert.equal((await rpc.waitFor(message=>message.id==='second')).error.code,-32013)
  rpc.child.stdin.end()
  await once(rpc.child, 'close')
})

test('all diagnostic budgets and invalid content preserve business execution', async () => {
  const rpc = start()
  await initialize(rpc)
  const source = `export async function execute(ctx) {
    let effects = 0;
    const nested = depth => ctx.trace.span('nested', () => depth ? nested(depth - 1) : effects++);
    await nested(20);
    await ctx.trace.span('budgets', span => {
      for (let i=0;i<70;i++) span.setAttribute('key'+i,i);
      for (let i=0;i<140;i++) span.event('event',{i});
      for (let i=0;i<35;i++) span.content({type:'acme/test',version:1,data:{i}});
      const cyclic={};cyclic.self=cyclic;
      span.content(cyclic);
      span.content({type:'acme/test',version:1,data:{value:BigInt(1)}});
      span.content({type:'acme/test',version:1,data:{value:'x'.repeat(2*1024*1024)}});
    });
    for(let i=0;i<100;i++) await ctx.trace.span('item',()=>effects++);
    return {status:'completed',outputs:{main:[{json:{effects}}]}};
  }`
  rpc.send({id:'budgets',method:'node.execute',params:{invocationId:'budgets',runtimeSource:source,execution:{deadline:new Date(Date.now()+5000).toISOString()}}})
  const response=await rpc.waitFor(message=>message.id==='budgets')
  assert.equal(response.result.outputs.main[0].json.effects,101)
  assert.ok(response.result.traceDiagnostics.dropped > 50)
  assert.ok(response.result.trace.length <= 64)
  rpc.child.stdin.end()
  await once(rpc.child,'close')
})

test('accepts fragmented transport writes and contains plugin stdout pollution', async () => {
  const rpc = start()
  const initialize = `${JSON.stringify({ jsonrpc: '2.0', id: 'fragmented-init', method: 'runner.initialize', params: { protocolVersion: 2, sdkApiVersion: 2 } })}\n`
  rpc.writeRaw(initialize.slice(0, 17))
  await new Promise((resolve) => setTimeout(resolve, 5))
  rpc.writeRaw(initialize.slice(17))
  assert.equal((await rpc.waitFor((message) => message.id === 'fragmented-init')).result.protocolVersion, 2)
  rpc.send({
    id: 'pollution', method: 'node.execute', params: {
      invocationId: 'pollution',
      runtimeSource: 'export async function execute(){process.stdout.write("not-json\\n");return {status:"completed",outputs:{main:[]}}}',
      execution: { deadline: new Date(Date.now() + 5_000).toISOString() },
    },
  })
  assert.deepEqual((await rpc.waitFor((message) => message.id === 'pollution')).result.outputs, { main: [] })
  rpc.child.stdin.end()
  await once(rpc.child, 'close')
})
