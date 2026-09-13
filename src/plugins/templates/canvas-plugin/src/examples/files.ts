import type { ArtifactRef, ExecuteContext } from '@agentx/plugin-sdk'

/** Stream an authorized input through the invocation directory without RPC file bytes. */
export async function copyArtifact(context: ExecuteContext, input: ArtifactRef): Promise<ArtifactRef> {
  const { createReadStream, createWriteStream } = await import('node:fs')
  const { pipeline } = await import('node:stream/promises')
  const local = await context.artifacts.read(input)
  await pipeline(createReadStream(local.path), createWriteStream('copy.bin'))
  return context.artifacts.put({ path: 'copy.bin', fileName: input.fileName, contentType: input.contentType })
}
