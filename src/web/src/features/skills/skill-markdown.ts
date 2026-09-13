export type SkillMarkdownDocument = {
  body: string
  description: string
}

const FRONTMATTER_PATTERN = /^---\r?\n([\s\S]*?)\r?\n---(?:\r?\n)?/

export function parseSkillMarkdown(markdown: string, fallbackDescription = ''): SkillMarkdownDocument {
  const normalized = markdown.replace(/\r\n/g, '\n')
  const match = normalized.match(FRONTMATTER_PATTERN)
  if (!match) return { body: markdown, description: fallbackDescription }

  const descriptionLine = match[1].split('\n').find((line) => line.startsWith('description:'))
  const description = descriptionLine ? parseYamlScalar(descriptionLine.slice('description:'.length).trim()) : fallbackDescription
  return { body: normalized.slice(match[0].length).replace(/^\n/, ''), description }
}

export function serializeSkillMarkdown(name: string, description: string, body: string): string {
  const normalizedBody = body.replace(/\r\n/g, '\n').replace(/^\n+/, '')
  return `---\nname: ${JSON.stringify(name)}\ndescription: ${JSON.stringify(description.trim())}\n---\n\n${normalizedBody}`
}

function parseYamlScalar(value: string): string {
  if (value.startsWith('"') && value.endsWith('"')) {
    try { return JSON.parse(value) as string } catch { return value.slice(1, -1) }
  }
  if (value.startsWith("'") && value.endsWith("'")) return value.slice(1, -1).replace(/''/g, "'")
  return value
}
