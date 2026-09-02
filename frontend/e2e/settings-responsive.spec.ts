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

test('task-based Settings controls stay inside their panels at every supported width', async ({ page }) => {
  for (const width of [761, 800, 920, 960, 1024, 1280, 1440, 1920]) {
    await page.setViewportSize({ width, height: 900 })
    await page.goto('/')
    await page.getByRole('button', { name: 'Settings', exact: true }).click()
    await expect(page.getByRole('group', { name: 'Whisper acceleration' })).toBeVisible()

    const geometry = await page.evaluate(() => {
      const fits = (node: HTMLElement) => {
        const section = node.closest('section')
        if (!section) throw new Error('Settings control is not contained in a section')
        return node.scrollWidth <= node.clientWidth &&
          node.getBoundingClientRect().right <= section.getBoundingClientRect().right + 0.5
      }
      const panels = [...document.querySelectorAll<HTMLElement>('.settings-section')]
      return {
        panelsFit: panels.every((panel) => panel.scrollWidth <= panel.clientWidth),
        controlsFit: [...document.querySelectorAll<HTMLElement>('.settings-section .segmented-control')]
          .every(fits),
        rowsFit: [...document.querySelectorAll<HTMLElement>('.settings-section .setting-row, .settings-section .setting-line')]
          .every(fits),
      }
    })

    expect(geometry, `settings at ${width}x900`).toEqual({
      panelsFit: true,
      controlsFit: true,
      rowsFit: true,
    })
  }
})

test('the last-used processing readout holds its longest reason inside Diagnostics', async ({ page }) => {
  const longest = 'CPU · GPU asked for, the device is disabled after a failure'

  for (const width of [761, 800, 920, 960, 1024, 1280, 1440, 1920]) {
    await page.setViewportSize({ width, height: 900 })
    await page.goto('/')
    await page.getByRole('button', { name: 'Settings', exact: true }).click()
    await expect(page.getByText('Last used processing', { exact: true })).toBeVisible()

    const geometry = await page.evaluate((text) => {
      const panel = document.querySelector<HTMLElement>('section[aria-label="Setup and diagnostics"]')
      if (!panel) throw new Error('Setup and diagnostics panel is missing')
      const line = [...panel.querySelectorAll<HTMLElement>('.setting-line')].find(
        (node) => node.querySelector('strong')?.textContent === 'Last used processing',
      )
      if (!line) throw new Error('Last used processing row is missing')
      const value = line.querySelector('span')
      if (!value) throw new Error('Last used processing value is missing')
      value.textContent = text
      const edge = panel.getBoundingClientRect().right
      return {
        rendered: value.textContent,
        panelFits: panel.scrollWidth <= panel.clientWidth,
        lineFits:
          line.scrollWidth <= line.clientWidth &&
          line.getBoundingClientRect().right <= edge + 0.5,
      }
    }, longest)

    expect(geometry, `processing readout at ${width}x900`).toEqual({
      rendered: longest,
      panelFits: true,
      lineFits: true,
    })
  }
})
