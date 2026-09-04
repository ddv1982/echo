import { expect, test } from '@playwright/test'

for (const theme of ['light', 'dark']) {
  test(`navigation stays labeled and reachable in ${theme} mode`, async ({ page }) => {
    await page.addInitScript((mode) => localStorage.setItem('echo-theme', mode), theme)
    for (const width of [390, 680, 681, 760, 920, 1280]) {
      await page.setViewportSize({ width, height: 680 })
      await page.goto('/')
      const nav = page.getByRole('navigation', { name: 'Echo sections' })
      for (const name of ['Home', 'History', 'Dictionary', 'Settings']) {
        const button = nav.getByRole('button', { name, exact: true })
        await expect(button.locator('span')).toBeVisible()
        await button.click()
        await expect(button).toHaveAttribute('aria-current', 'page')
        if (name !== 'Home') await expect(page.getByRole('heading', { name, exact: true })).toBeVisible()
      }
      const fits = await page.evaluate(() => {
        const buttons = [...document.querySelectorAll<HTMLButtonElement>('.topbar button')]
        const bounds = buttons.map((button) => button.getBoundingClientRect())
        return bounds.every((a, index) => a.left >= 0 && a.right <= innerWidth && a.height >= 32 &&
          bounds.every((b, other) => index === other || a.right <= b.left || b.right <= a.left || a.bottom <= b.top || b.bottom <= a.top))
      })
      expect(fits, `header controls at ${width}px`).toBe(true)
    }
  })

  test(`secondary text stays readable on every ${theme} surface`, async ({ page }) => {
    await page.addInitScript((mode) => localStorage.setItem('echo-theme', mode), theme)
    await page.goto('/')
    await expect(page.locator('html')).toHaveAttribute('data-theme', theme)
    const ratios = await page.evaluate(() => {
      const sample = document.createElement('span')
      document.body.append(sample)
      const luminance = (token: string) => {
        sample.style.color = `hsl(var(${token}))`
        const [red, green, blue] = getComputedStyle(sample).color.match(/[\d.]+/g)?.slice(0, 3).map(Number) ?? []
        if (red === undefined || green === undefined || blue === undefined) throw new Error(`Cannot resolve ${token}`)
        const linear = (channel: number) => {
          const value = channel / 255
          return value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4
        }
        return linear(red) * 0.2126 + linear(green) * 0.7152 + linear(blue) * 0.0722
      }
      const text = luminance('--text-tertiary')
      const result = ['--surface-app', '--surface-card', '--surface-raised'].map((surface) => {
        const background = luminance(surface)
        return { surface, ratio: (Math.max(text, background) + 0.05) / (Math.min(text, background) + 0.05) }
      })
      sample.remove()
      return result
    })
    for (const { surface, ratio } of ratios) expect(ratio, `${theme} ${surface}`).toBeGreaterThanOrEqual(4.5)
  })
}

test('recording, processing and history work with the keyboard', async ({ page }) => {
  await page.goto('/')
  const start = page.getByRole('button', { name: 'Start recording', exact: true })
  await start.focus()
  await expect(start).toBeFocused()
  await page.keyboard.press('Enter')
  const stop = page.getByRole('button', { name: 'Stop and transcribe', exact: true })
  await expect(stop).toBeFocused()
  await expect(page.getByRole('heading', { name: 'Listening…', exact: true })).toBeVisible()
  await expect(page.locator('.readout-timer')).toContainText('/ 10:00')
  await page.keyboard.press('Enter')
  await expect(page.getByRole('button', { name: 'Transcribing', exact: true })).toBeDisabled()
  await expect(start).toBeEnabled()
  await page.getByRole('button', { name: 'View history', exact: true }).click()
  await expect(page.getByRole('heading', { name: 'History', exact: true })).toBeVisible()
  await page.getByRole('textbox', { name: 'Search history' }).fill('no transcript matches this phrase')
  await expect(page.getByText('No matching transcripts', { exact: true })).toBeVisible()
})

test('dictionary edits and voice dialog preserve focus through the redesigned shell', async ({ page }) => {
  await page.goto('/')
  await page.getByRole('button', { name: 'Dictionary', exact: true }).click()
  await page.getByRole('textbox', { name: 'What Echo hears', exact: true }).fill('design studio')
  await page.getByRole('textbox', { name: 'What Echo should write', exact: true }).fill('Design Studio')
  await page.getByRole('button', { name: 'Add', exact: true }).click()
  const remove = page.getByRole('button', { name: 'Remove Design Studio', exact: true })
  await expect(remove).toBeVisible()
  await page.getByRole('button', { name: 'Home', exact: true }).click()
  await page.getByRole('button', { name: 'Dictionary', exact: true }).click()
  await expect(remove).toBeVisible()
  const trigger = page.getByRole('button', { name: 'Teach by voice', exact: true })
  await trigger.click()
  await expect(page.getByRole('dialog', { name: 'Teach Echo by voice' })).toBeVisible()
  await page.keyboard.press('Escape')
  await expect(trigger).toBeFocused()
  await remove.click()
  await expect(remove).toHaveCount(0)
})

test('reduced motion stops the processing animation', async ({ page }) => {
  await page.emulateMedia({ reducedMotion: 'reduce' })
  await page.goto('/')
  await page.getByRole('button', { name: 'Start recording', exact: true }).click()
  await page.getByRole('button', { name: 'Stop and transcribe', exact: true }).click()
  const spinner = page.locator('.processing-icon')
  await expect(spinner).toBeVisible()
  const duration = await spinner.evaluate((node) => getComputedStyle(node).animationDuration)
  expect(parseFloat(duration)).toBeLessThanOrEqual(0.00001)
})

test('long dictionary phrases wrap inside a narrow window', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 })
  await page.goto('/')
  await page.getByRole('button', { name: 'Dictionary', exact: true }).click()
  await page.getByRole('textbox', { name: 'What Echo hears', exact: true }).fill('s'.repeat(200))
  await page.getByRole('textbox', { name: 'What Echo should write', exact: true }).fill('W'.repeat(200))
  await page.getByRole('button', { name: 'Add', exact: true }).click()
  await expect(page.getByRole('button', { name: `Remove ${'W'.repeat(200)}`, exact: true })).toBeVisible()
  const fits = await page.locator('.dictionary-row').last().evaluate((node) =>
    node.scrollWidth <= node.clientWidth && document.documentElement.scrollWidth <= innerWidth)
  expect(fits).toBe(true)
})
