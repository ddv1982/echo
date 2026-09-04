import { mkdir } from 'node:fs/promises'
import { resolve } from 'node:path'
import { chromium } from '@playwright/test'

const destination = resolve(process.argv[2] ?? resolve(import.meta.dirname, '../test-results/design'))
const baseURL = process.env.ECHO_PREVIEW_URL ?? 'http://127.0.0.1:4178'
await mkdir(destination, { recursive: true })
const browser = await chromium.launch()
try {
  for (const theme of ['light', 'dark']) {
    for (const [width, height] of [[920, 680], [760, 560], [390, 844], [1440, 1000]]) {
      const page = await browser.newPage({ viewport: { width, height }, reducedMotion: 'reduce' })
      await page.addInitScript((mode) => localStorage.setItem('echo-theme', mode), theme)
      await page.goto(baseURL)
      await page.getByRole('button', { name: 'Start recording', exact: true }).waitFor()
      await page.getByRole('region', { name: 'Finish setup' }).waitFor()
      await page.evaluate(() => document.fonts.ready)
      for (const view of ['Home', 'History', 'Dictionary', 'Settings']) {
        await page.getByRole('button', { name: view, exact: true }).click()
        if (view === 'Settings') await page.getByRole('button', { name: 'Test selected', exact: true }).waitFor()
        await page.screenshot({ path: resolve(destination, `${theme}-${width}-${view.toLowerCase()}.png`), fullPage: true })
      }
      await page.evaluate(() => {
        localStorage.setItem('echo-shortcut-verified-at', String(Math.floor(Date.now() / 1000)))
        localStorage.setItem('echo-shortcut-verified-identity', 'portal:Super+Alt+Space')
      })
      await page.reload()
      await page.getByRole('button', { name: 'Start recording', exact: true }).waitFor()
      await page.getByRole('region', { name: 'Finish setup' }).waitFor({ state: 'hidden' })
      await page.screenshot({ path: resolve(destination, `${theme}-${width}-home-ready.png`), fullPage: true })
      await page.getByRole('button', { name: 'Start recording', exact: true }).click()
      await page.getByRole('button', { name: 'Stop and transcribe', exact: true }).waitFor()
      await page.screenshot({ path: resolve(destination, `${theme}-${width}-recording.png`), fullPage: true })
      await page.close()
    }
  }
} finally {
  await browser.close()
}
console.log(`Design screenshots saved to ${destination}`)
