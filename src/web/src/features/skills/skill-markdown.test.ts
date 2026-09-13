import { describe, expect, it } from 'vitest'

import { parseSkillMarkdown, serializeSkillMarkdown } from './skill-markdown'

describe('skill markdown document', () => {
  it('keeps description outside the rich-text body', () => {
    const markdown = serializeSkillMarkdown('browser-helper', 'Use browser tools', '# Instructions\n\n- Open the page')
    expect(parseSkillMarkdown(markdown)).toEqual({
      description: 'Use browser tools',
      body: '# Instructions\n\n- Open the page',
    })
  })

  it('supports quoted descriptions containing YAML punctuation', () => {
    expect(parseSkillMarkdown('---\nname: "helper"\ndescription: "Use: browser tools"\n---\n\n# Body')).toEqual({
      description: 'Use: browser tools',
      body: '# Body',
    })
  })

  it('falls back to the entity description for legacy drafts', () => {
    expect(parseSkillMarkdown('# Body', 'Legacy description')).toEqual({ description: 'Legacy description', body: '# Body' })
  })
})
