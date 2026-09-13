export const WORKSPACE_ROOT_PARENT = '__workspace_root__'

export function workspaceParentId(value: string): string | null {
  return !value || value === WORKSPACE_ROOT_PARENT ? null : value
}

export function workspaceParentValue(parentId?: string | null): string {
  return parentId ?? WORKSPACE_ROOT_PARENT
}

export function skillReferenceMarkdown(sourcePath: string, targetPath: string, targetName: string, mimeType?: string | null): string {
  const path = relativePath(sourcePath, targetPath)
  return mimeType?.startsWith('image/') ? `![${targetName}](${path})` : `[${targetName}](${path})`
}

function relativePath(source: string, target: string) {
  const from = source.split('/').slice(0, -1)
  const to = target.split('/')
  let common = 0
  while (from[common] === to[common] && common < from.length && common < to.length) common += 1
  return [...Array.from({ length: from.length - common }, () => '..'), ...to.slice(common)].join('/')
}
