export type { ReferenceCatalog, ReferenceEntry, ReferenceNamespace } from '../../model/types'

export type ReferenceSelection = {
  expression: string
  entry: import('../../model/types').ReferenceEntry
}
