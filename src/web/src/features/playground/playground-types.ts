import type { ArtifactReference, JsonSchema } from '../../shared/components/schema-form'

export type ChatMapping = {
  questionInput: string
  fileInput: string | null
  answerOutput: string
  answerFilesOutput: string | null
}

export type PlaygroundConfig = {
  applicationId: string
  deploymentId: string
  deploymentVersion: number
  version: number
  mapping: ChatMapping | null
  publishedVersion: number | null
  publishStatus: 'active' | 'publishing' | 'failed'
  errorCode: string | null
  errorMessage: string | null
}

export type PlaygroundDeployment = {
  id: string
  applicationId: string
  workflowVersionId: string
  workflowVersionNumber: number
  sequenceNumber: number
  inputSchema: JsonSchema
  outputSchema: JsonSchema
  status: string
}

export type UploadedArtifact = ArtifactReference & {
  contentType: string
  sizeBytes: number
  sha256: string
}
