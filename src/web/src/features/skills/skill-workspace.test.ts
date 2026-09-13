import { describe, expect, it } from 'vitest'

import { skillReferenceMarkdown, WORKSPACE_ROOT_PARENT, workspaceParentId, workspaceParentValue } from './skill-workspace'

describe('Skill workspace form values', () => {
  it('keeps the workspace root visible while sending a null parent to the API', () => {
    expect(workspaceParentValue(null)).toBe(WORKSPACE_ROOT_PARENT)
    expect(workspaceParentId(WORKSPACE_ROOT_PARENT)).toBeNull()
    expect(workspaceParentId('directory-id')).toBe('directory-id')
  })

  it('builds file and image references relative to the edited document', () => {
    expect(skillReferenceMarkdown('docs/guide.md', 'assets/sample.txt', 'sample.txt', 'text/plain')).toBe('[sample.txt](../assets/sample.txt)')
    expect(skillReferenceMarkdown('docs/guide.md', 'docs/diagram.png', 'diagram.png', 'image/png')).toBe('![diagram.png](diagram.png)')
  })
})
