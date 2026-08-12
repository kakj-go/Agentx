import { writeFile } from 'node:fs/promises'
import { resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

import openapiTS, { astToString } from 'openapi-typescript'

const schemaUrl = new URL('../../../openapi/platform-api.json', import.meta.url)
const defaultOutputUrl = new URL('../src/shared/api/generated.ts', import.meta.url)
const outputPath = process.argv[2]
  ? resolve(process.argv[2])
  : fileURLToPath(defaultOutputUrl)

const generated = astToString(await openapiTS(schemaUrl))
const componentStart = generated.indexOf('export interface components {')
const componentEnd = generated.indexOf('\nexport type $defs', componentStart)

if (componentStart < 0 || componentEnd < 0) {
  throw new Error('Unable to locate the generated OpenAPI components interface')
}

const header = `/** Generated schema components from openapi/platform-api.json. Do not edit. */\n`
const components = generated
  .slice(componentStart, componentEnd)
  .replace(/^[ \t]*\/\*\* Format: [^\r\n]*\*\/\r?\n/gm, '')

await writeFile(outputPath, `${header}${components}\n`, 'utf8')
