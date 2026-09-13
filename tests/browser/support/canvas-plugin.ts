import { expect, type Locator, type Page } from '@playwright/test'

const password = 'agentx-e2e-admin-password'

export async function login(page: Page) {
  const status = await page.request.get('/api/v1/bootstrap/status')
  if (((await status.json()) as { required: boolean }).required) {
    await page.goto('/setup')
    await page.getByLabel('公司名称').fill('Agentx E2E')
    await page.getByLabel('用户名').fill('admin')
    await page.getByLabel('管理员姓名').fill('E2E Admin')
    await page.getByLabel('密码').fill(password)
    const response = page.waitForResponse((value) => value.url().endsWith('/api/v1/bootstrap'))
    await page.getByRole('button', { name: '初始化并进入工作台' }).click()
    return ((await (await response).json()) as { accessToken: string }).accessToken
  }
  await page.goto('/login')
  await page.getByLabel('用户名').fill('admin')
  await page.getByLabel('密码').fill(password)
  const response = page.waitForResponse((value) => value.url().endsWith('/api/v1/auth/login'))
  await page.getByRole('button', { name: '登录' }).click()
  return ((await (await response).json()) as { accessToken: string }).accessToken
}


export async function connect(page: Page, source: Locator, sourceHandle: string, target: Locator, targetHandle: string) {
  const edges = page.locator('.react-flow__edge'); const edgeCount = await edges.count()
  const from = source.locator(`.react-flow__handle.source[data-handleid="${sourceHandle}"]`)
  const to = target.locator(`.react-flow__handle.target[data-handleid="${targetHandle}"]`)
  const drag = async (attempts: number) => {
    for (let attempt = 0; attempt < attempts; attempt += 1) {
      if (await edges.count() === edgeCount + 1) return
      await page.keyboard.press('Escape')
      await expect(from).toBeVisible(); await expect(to).toBeVisible()
      const fromBox = await from.boundingBox(); const toBox = await to.boundingBox()
      if (!fromBox || !toBox) throw new Error('Connection handle is not measurable')
      await page.mouse.move(fromBox.x + fromBox.width / 2, fromBox.y + fromBox.height / 2)
      await page.mouse.down()
      await page.mouse.move(fromBox.x + fromBox.width / 2 + 12, fromBox.y + fromBox.height / 2, { steps: 4 })
      await page.waitForTimeout(100)
      await page.mouse.move(toBox.x + toBox.width / 2, toBox.y + toBox.height / 2, { steps: 30 })
      await page.waitForTimeout(100)
      await page.mouse.up()
      await expect.poll(() => edges.count(), { timeout: 3_000 }).toBe(edgeCount + 1).catch(() => undefined)
    }
  }
  await drag(5)
  if (await edges.count() === edgeCount) {
    await page.getByRole('button', { name: /^(自动布局|Auto layout)$/i }).click({ force: true })
    await page.waitForTimeout(750)
    await page.getByRole('button', { name: /^(适应画布|Fit view)$/i }).click({ force: true })
    await page.waitForTimeout(500)
    await drag(5)
  }
  for (let attempt = 0; attempt < 2 && await edges.count() === edgeCount; attempt += 1) {
    await from.click({ force: true })
    await to.click({ force: true })
    await expect.poll(() => edges.count(), { timeout: 3_000 }).toBe(edgeCount + 1).catch(async () => { await page.keyboard.press('Escape') })
  }
  await expect(edges).toHaveCount(edgeCount + 1)
}

