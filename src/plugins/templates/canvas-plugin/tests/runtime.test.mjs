import assert from 'node:assert/strict'
import { spawn } from 'node:child_process'
import { once } from 'node:events'
import { existsSync } from 'node:fs'
import { readFile } from 'node:fs/promises'
import { createInterface } from 'node:readline'
import test from 'node:test'
import { fileURLToPath } from 'node:url'

test('executes the built plugin with the production Agentx Runner protocol', async () => {
  const vendoredRunner = new URL('../vendor/plugin-runner/runner.mjs', import.meta.url)
  const runner = existsSync(vendoredRunner) ? vendoredRunner : new URL('../../../packages/plugin-runner/runner.mjs', import.meta.url)
  const child = spawn(process.execPath, [fileURLToPath(runner)], { stdio: ['pipe', 'pipe', 'pipe'] })
  const messages = []
  createInterface({ input: child.stdout, crlfDelay: Infinity }).on('line', (line) => messages.push(JSON.parse(line)))
  const send = (message) => child.stdin.write(`${JSON.stringify({ jsonrpc: '2.0', ...message })}\n`)
  send({ id: 'init', method: 'runner.initialize', params: { protocolVersion: 1, sdkApiVersion: 1 } })
  send({ id: 'execute', method: 'node.execute', params: {
    invocationId: 'template-test', runtimeSource: await readFile(new URL('../dist/runtime/entry.js', import.meta.url), 'utf8'),
    inputs: { main: [{ json: { id: 1 } }] }, parameters: { label: 'customer' }, perItemParameters: [],
    execution: { nodeType: 'acme.json_mapper', nodeVersion: 1, packageId: 'acme/json-mapper', packageVersion: '1.0.0', bundleDigest: `sha256:${'a'.repeat(64)}`, executionId: 'execution', nodeExecutionId: 'node', attemptId: 'attempt', runIndex: 0, iterationIndex: 0, idempotencyKey: 'attempt:1', deadline: new Date(Date.now() + 5_000).toISOString() },
  } })
  child.stdin.end()
  await once(child, 'close')
  const initialized = messages.find((message) => message.id === 'init')
  const executed = messages.find((message) => message.id === 'execute')
  assert.equal(initialized.result.protocolVersion, 1)
  assert.deepEqual(executed.result.outputs.main[0].json, { id: 1, label: 'customer' })
  assert.equal(executed.result.trace[0].contents[0].type, 'acme.json-mapper/summary')
})
