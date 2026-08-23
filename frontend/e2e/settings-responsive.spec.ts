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
