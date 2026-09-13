import { useCallback } from 'react'

import { apiRequestBlob } from '../../shared/api/client'
import { useToast } from '../../shared/ui/toast'

export function useExecutionArtifactDownload(executionId?: string) {
  const { showToast } = useToast()

  return useCallback(async (artifactId: string) => {
    if (!executionId) return
    try {
      const blob = await apiRequestBlob(`/executions/${executionId}/artifacts/${artifactId}`)
      const url = URL.createObjectURL(blob)
      const anchor = document.createElement('a')
      anchor.href = url
      anchor.download = artifactId
      anchor.click()
      URL.revokeObjectURL(url)
    } catch (error) {
      showToast(error instanceof Error ? error.message : String(error))
    }
  }, [executionId, showToast])
}
