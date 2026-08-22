import { fireEvent, render, screen } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { i18n } from '../../app/i18n'
import { DateTimePicker } from './date-time-picker'

const messages = {
  time: '时间',
  now: '现在',
  clear: '清除',
  confirm: '确认',
  invalidTime: '请输入有效时间，格式为 HH:mm。',
  outOfRange: '所选时间不在允许范围内。',
}

describe('DateTimePicker', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('zh-CN')
  })

  it('opens from the whole trigger and commits the selected local date and time', () => {
    const onChange = vi.fn()
    const initial = new Date(2026, 7, 22, 9, 15)
    render(<DateTimePicker label="开始时间下限" messages={messages} onChange={onChange} value={initial} />)

    fireEvent.click(screen.getByRole('button', { name: '开始时间下限' }))
    expect(screen.getByRole('grid')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: /2026年8月23日/ }))
    fireEvent.change(screen.getByRole('textbox', { name: '时间' }), { target: { value: '14:35' } })
    fireEvent.click(screen.getByRole('button', { name: '确认' }))

    const selected = onChange.mock.calls[0][0] as Date
    expect([
      selected.getFullYear(),
      selected.getMonth(),
      selected.getDate(),
      selected.getHours(),
      selected.getMinutes(),
    ]).toEqual([2026, 7, 23, 14, 35])
    expect(screen.queryByRole('grid')).not.toBeInTheDocument()
  })

  it('rejects invalid time text and values outside the allowed range', () => {
    const onChange = vi.fn()
    const initial = new Date(2026, 7, 22, 9, 15)
    render(<DateTimePicker label="结束时间上限" max={new Date(2026, 7, 22, 10, 0)} messages={messages} onChange={onChange} value={initial} />)

    fireEvent.click(screen.getByRole('button', { name: '结束时间上限' }))
    const time = screen.getByRole('textbox', { name: '时间' })
    fireEvent.change(time, { target: { value: '25:00' } })
    fireEvent.click(screen.getByRole('button', { name: '确认' }))
    expect(screen.getByRole('alert')).toHaveTextContent('请输入有效时间')

    fireEvent.change(time, { target: { value: '10:30' } })
    fireEvent.click(screen.getByRole('button', { name: '确认' }))
    expect(screen.getByRole('alert')).toHaveTextContent('所选时间不在允许范围内')
    expect(onChange).not.toHaveBeenCalled()
  })

  it('clears an existing value from the popover', () => {
    const onChange = vi.fn()
    render(<DateTimePicker label="开始时间下限" messages={messages} onChange={onChange} value={new Date(2026, 7, 22, 9, 15)} />)

    fireEvent.click(screen.getByRole('button', { name: '开始时间下限' }))
    fireEvent.click(screen.getByRole('button', { name: '清除' }))

    expect(onChange).toHaveBeenCalledWith(undefined)
    expect(screen.queryByRole('grid')).not.toBeInTheDocument()
  })
})
