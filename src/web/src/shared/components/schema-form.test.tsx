import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { useState } from 'react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { i18n } from '../../app/i18n'
import { SchemaFormWorkspace, schemaDefaults, validateSchemaValues, type JsonSchema } from './schema-form'

const schema: JsonSchema = {
  type: 'object',
  required: ['prompt', 'documents'],
  properties: {
    prompt: { type: 'string', title: '问题', default: '默认问题' },
    count: { type: 'integer', default: 2 },
    options: { type: 'object', properties: { enabled: { type: 'boolean', default: true } } },
    documents: { type: 'array', title: '文件', 'x-agentx-artifact': true, 'x-agentx-artifact-array': true },
  },
}

describe('shared schema form', () => {
  beforeEach(async () => { await i18n.changeLanguage('zh-CN') })
  it('recursively applies defaults and validates required artifact arrays', () => {
    expect(schemaDefaults(schema)).toEqual({ prompt: '默认问题', count: 2, options: { enabled: true } })
    expect(validateSchemaValues(schema, schemaDefaults(schema))).toMatchObject({ documents: expect.any(String) })
  })

  it('renders complex fields, JSON mode, and uploads Artifact references', async () => {
    const upload = vi.fn(async (file: File) => ({ artifactId: 'artifact-1', fileName: file.name }))
    function Example() { const [value, setValue] = useState(schemaDefaults(schema)); return <><SchemaFormWorkspace onChange={setValue} onUpload={upload} schema={schema} value={value} /><output>{JSON.stringify(value)}</output></> }
    render(<Example />)
    expect(screen.getByDisplayValue('默认问题')).toBeVisible()
    fireEvent.change(screen.getByLabelText('文件'), { target: { files: [new File(['hello'], 'guide.txt')] } })
    await waitFor(() => expect(upload).toHaveBeenCalled())
    expect(await screen.findByText('guide.txt')).toBeVisible()
    fireEvent.click(screen.getByRole('button', { name: /JSON 模式/ }))
    expect((screen.getByLabelText('JSON 输入') as HTMLTextAreaElement).value).toContain('artifact-1')
  })

  it('keeps the field usable and reports Artifact upload failures', async () => {
    const upload = vi.fn(async () => { throw new Error('Artifact upload unavailable') })
    function Example() { const [value, setValue] = useState(schemaDefaults(schema)); return <SchemaFormWorkspace onChange={setValue} onUpload={upload} schema={schema} value={value} /> }
    render(<Example />)

    fireEvent.change(screen.getByLabelText('文件'), { target: { files: [new File(['hello'], 'failed.txt')] } })

    expect(await screen.findByText('Artifact upload unavailable')).toBeVisible()
    expect(screen.getByLabelText('文件')).toBeEnabled()
  })

  it('rejects Artifact files that violate schema limits before upload', async () => {
    const upload = vi.fn(async (file: File) => ({ artifactId: 'unexpected', fileName: file.name }))
    const constrained: JsonSchema = structuredClone(schema)
    constrained.properties!.documents = { ...constrained.properties!.documents, maxItems: 1, 'x-agentx-content-types': ['image/*'], 'x-agentx-max-size-bytes': 4, 'x-agentx-max-total-size-bytes': 4 }
    function Example() { const [value, setValue] = useState(schemaDefaults(constrained)); return <SchemaFormWorkspace onChange={setValue} onUpload={upload} schema={constrained} value={value} /> }
    render(<Example />)

    fireEvent.change(screen.getByLabelText('文件'), { target: { files: [new File(['hello'], 'guide.txt', { type: 'text/plain' })] } })

    expect(await screen.findByText(/不支持文件 guide.txt/)).toBeVisible()
    expect(upload).not.toHaveBeenCalled()
  })

  it('renders every supported scalar, collection, and Artifact control', () => {
    const complete: JsonSchema = {
      type: 'object',
      properties: {
        title: { type: 'string', title: '标题' },
        description: { type: 'string', title: '描述', multiline: true },
        mode: { type: 'string', title: '模式', enum: ['fast', 'safe'] },
        ratio: { type: 'number', title: '比例' },
        retries: { type: 'integer', title: '重试次数' },
        enabled: { type: 'boolean', title: '启用' },
        metadata: { type: 'object', title: '元数据', properties: { region: { type: 'string' } } },
        tags: { type: 'array', title: '标签', items: { type: 'string' } },
        cover: { type: 'object', title: '封面', 'x-agentx-artifact': true },
        documents: { type: 'array', title: '文档', 'x-agentx-artifact': true, 'x-agentx-artifact-array': true },
      },
    }
    render(<SchemaFormWorkspace onChange={() => undefined} onUpload={async (file) => ({ artifactId: file.name })} schema={complete} value={{}} />)

    expect(screen.getByLabelText('标题')).toHaveAttribute('type', 'text')
    expect(screen.getByLabelText('描述').tagName).toBe('TEXTAREA')
    expect(screen.getByRole('combobox', { name: '模式' })).toBeVisible()
    expect(screen.getByLabelText('比例')).toHaveAttribute('step', 'any')
    expect(screen.getByLabelText('重试次数')).toHaveAttribute('step', '1')
    expect(screen.getByLabelText('启用')).toHaveAttribute('type', 'checkbox')
    expect(screen.getByLabelText('元数据').tagName).toBe('TEXTAREA')
    expect(screen.getByLabelText('标签').tagName).toBe('TEXTAREA')
    expect(screen.getByLabelText('封面')).not.toHaveAttribute('multiple')
    expect(screen.getByLabelText('文档')).toHaveAttribute('multiple')
  })
})
