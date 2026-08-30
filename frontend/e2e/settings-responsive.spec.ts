import { expect, test } from '@playwright/test'

const widths = [760, 761, 800, 920, 959, 960, 961, 1024]

for (const theme of ['light', 'dark'] as const) {
  test(`Settings fits every supported edge in ${theme} mode`, async ({ page }) => {
    await page.addInitScript((mode) => localStorage.setItem('echo-theme', mode), theme)

    for (const width of widths) {
      await page.setViewportSize({ width, height: 600 })
      await page.goto('/')
      await page.getByRole('button', { name: 'Settings', exact: true }).click()
      await expect(page.getByRole('radio', { name: /Jabra Elite 8 Active/ })).toBeVisible()
      await expect(page.getByRole('button', { name: 'Test selected' })).toBeVisible()
      await expect(page.getByText('Ready to dictate')).toBeVisible()
      await expect(page.locator('html')).toHaveAttribute('data-theme', theme)

      const geometry = await page.evaluate(() => ({
        documentFits:
          document.documentElement.scrollWidth <= document.documentElement.clientWidth,
        surfacesFit: [...document.querySelectorAll<HTMLElement>('[data-settings-surface]')]
          .every((node) => node.scrollWidth <= node.clientWidth),
        disclosuresClosed: [...document.querySelectorAll<HTMLDetailsElement>('.settings-disclosure')]
          .every((node) => !node.open),
      }))

      expect(geometry, `${theme} at ${width}x600`).toEqual({
        documentFits: true,
        surfacesFit: true,
        disclosuresClosed: true,
      })
    }
  })
}

test('Advanced controls stay inside the panel at every supported width', async ({ page }) => {
  for (const width of [761, 800, 920, 960, 1024, 1280, 1440, 1920]) {
    await page.setViewportSize({ width, height: 900 })
    await page.goto('/')
    await page.getByRole('button', { name: 'Settings', exact: true }).click()
    await page.locator('.advanced-section > summary').click()
    await expect(page.getByRole('group', { name: 'Whisper acceleration' })).toBeVisible()

    const geometry = await page.evaluate(() => {
      const panel = document.querySelector<HTMLElement>('.advanced-section')!
      const edge = panel.getBoundingClientRect().right
      const fits = (node: HTMLElement) =>
        node.scrollWidth <= node.clientWidth && node.getBoundingClientRect().right <= edge + 0.5
      return {
        panelFits: panel.scrollWidth <= panel.clientWidth,
        controlsFit: [...panel.querySelectorAll<HTMLElement>('.segmented-control')].every(fits),
        rowsFit: [...panel.querySelectorAll<HTMLElement>('.setting-row, .setting-line')].every(fits),
      }
    })

    expect(geometry, `advanced at ${width}x900`).toEqual({
      panelFits: true,
      controlsFit: true,
      rowsFit: true,
    })
  }
})

// The Acceleration readout is the one Advanced line whose text length is set by
// a runtime outcome rather than by copy we control at build time. The longest
// skip reason is written into the rendered line and measured in the same task,
// before React can re-render over it.
test('the Acceleration readout holds its longest skip reason inside the panel', async ({ page }) => {
  const longest = 'CPU · GPU asked for, the device is disabled after a failure'

  for (const width of [761, 800, 920, 960, 1024, 1280, 1440, 1920]) {
    await page.setViewportSize({ width, height: 900 })
    await page.goto('/')
    await page.getByRole('button', { name: 'Settings', exact: true }).click()
    await page.locator('.advanced-section > summary').click()
    await expect(page.getByText('Acceleration', { exact: true })).toBeVisible()

    const geometry = await page.evaluate((text) => {
      const panel = document.querySelector<HTMLElement>('.advanced-section')!
      const line = [...panel.querySelectorAll<HTMLElement>('.setting-line')].find(
        (node) => node.querySelector('strong')?.textContent === 'Acceleration',
      )!
      line.querySelector('span')!.textContent = text
      const edge = panel.getBoundingClientRect().right
      return {
        rendered: line.querySelector('span')!.textContent,
        panelFits: panel.scrollWidth <= panel.clientWidth,
        lineFits:
          line.scrollWidth <= line.clientWidth &&
          line.getBoundingClientRect().right <= edge + 0.5,
      }
    }, longest)

    expect(geometry, `acceleration readout at ${width}x900`).toEqual({
      rendered: longest,
      panelFits: true,
      lineFits: true,
    })
  }
})
