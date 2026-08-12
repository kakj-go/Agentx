export function insertAtSelection(value: string, insertion: string, start?: number | null, end?: number | null) {
  const from = start ?? value.length
  const to = end ?? from
  return { value: `${value.slice(0, from)}${insertion}${value.slice(to)}`, cursor: from + insertion.length }
}
