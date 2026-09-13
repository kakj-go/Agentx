import { writeFile } from 'node:fs/promises'
import { resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

import openapiTS, { astToString } from 'openapi-typescript'

const schemaUrl = new URL('../../../contracts/openapi/trigger-gateway.json', import.meta.url)
const defaultOutputUrl = new URL('../src/shared/api/generated-gateway.ts', import.meta.url)
const outputPath = process.argv[2] ? resolve(process.argv[2]) : fileURLToPath(defaultOutputUrl)
const generated = astToString(await openapiTS(schemaUrl))
const componentStart = generated.indexOf('export interface components {')
const componentEnd = generated.indexOf('\nexport type $defs', componentStart)

if (componentStart < 0 || componentEnd < 0) throw new Error('Unable to locate Gateway components')

const header = `/**
 * Auto-generated from contracts/openapi/trigger-gateway.json. Do not edit directly.
 */

`
await writeFile(outputPath, `${header}${generated.slice(componentStart, componentEnd)}\n`, 'utf8')
