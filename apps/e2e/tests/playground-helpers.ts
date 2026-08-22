import { expect, type Page } from '@playwright/test'

type JsonSchema = {
  type?: string
  properties?: Record<string, JsonSchema>
  'x-agentx-artifact'?: boolean
  'x-agentx-artifact-array'?: boolean
  'x-agentx-sensitive'?: boolean
}

type Deployment = {
  id: string
  inputSchema: JsonSchema
  outputSchema: JsonSchema
}

type PlaygroundConfig = {
  version: number
  publishStatus: 'active' | 'publishing' | 'failed'
  errorCode?: string | null
  errorMessage?: string | null
}

export type ChatMapping = {
  questionInput: string
  fileInput: string | null
  answerOutput: string
  answerFilesOutput: string | null
}

export async function publishCompatibleChatMapping(page: Page, token: string, applicationId: string, deploymentId: string) {
  const headers = { Authorization: `Bearer ${token}` }
  const deploymentsResponse = await page.request.get(`/api/v1/applications/${applicationId}/deployments`, { headers })
  if (!deploymentsResponse.ok()) throw new Error(`Deployment query failed: ${await deploymentsResponse.text()}`)
  const deployment = (await deploymentsResponse.json() as Deployment[]).find((item) => item.id === deploymentId)
  expect(deployment, `Deployment ${deploymentId} is not visible`).toBeTruthy()

  const mapping = compatibleMapping(deployment!)
  await publishChatMapping(page, token, applicationId, deploymentId, mapping)
  return mapping
}

export async function publishChatMapping(page: Page, token: string, applicationId: string, deploymentId: string, mapping: ChatMapping | null) {
  const headers = { Authorization: `Bearer ${token}` }
  const configPath = `/api/v1/applications/${applicationId}/deployments/${deploymentId}/playground-config`
  const configResponse = await page.request.get(configPath, { headers })
  if (!configResponse.ok()) throw new Error(`Playground config query failed: ${await configResponse.text()}`)
  const config = await configResponse.json() as PlaygroundConfig
  const putResponse = await page.request.put(configPath, { headers, data: { expectedVersion: config.version, mapping } })
  if (!putResponse.ok()) throw new Error(`Playground config publish failed: ${await putResponse.text()}`)

  await expect.poll(async () => {
    const response = await page.request.get(configPath, { headers })
    if (!response.ok()) throw new Error(`Playground config poll failed: ${await response.text()}`)
    const current = await response.json() as PlaygroundConfig
    if (current.publishStatus === 'failed') throw new Error(`${current.errorCode ?? 'PLAYGROUND_MAPPING_PUBLISH_FAILED'}: ${current.errorMessage ?? ''}`)
    return current.publishStatus
  }, { timeout: 120_000, intervals: [500, 1_000, 2_000] }).toBe('active')
}

function compatibleMapping(deployment: Deployment): ChatMapping {
  const inputs = Object.entries(deployment.inputSchema.properties ?? {})
  const outputs = Object.entries(deployment.outputSchema.properties ?? {})
  const questionInput = inputs.find(([, field]) => field.type === 'string' && !field['x-agentx-artifact'])?.[0]
  const fileInput = inputs.find(([, field]) => field['x-agentx-artifact'])?.[0] ?? null
  const answerOutput = outputs.find(([, field]) => field.type === 'string' && !field['x-agentx-sensitive'])?.[0]
  const answerFilesOutput = outputs.find(([, field]) => field['x-agentx-artifact'])?.[0] ?? null
  if (!questionInput || !answerOutput) throw new Error('Deployment does not expose compatible question and answer fields')
  return { questionInput, fileInput, answerOutput, answerFilesOutput }
}
