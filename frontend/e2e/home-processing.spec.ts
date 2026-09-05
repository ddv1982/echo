import { expect, test } from '@playwright/test'

test('the preview blocks a second toggle while transcribing, then accepts the next recording', async ({ page }) => {
  await page.goto('/')

  const start = page.getByRole('button', { name: 'Start recording' })
  await expect(start).toBeEnabled()
  await start.click()

  const stop = page.getByRole('button', { name: 'Stop and transcribe' })
  await expect(stop).toBeEnabled()
  await stop.click()

  const processing = page.getByRole('button', { name: 'Processing recording' })
  await expect(processing).toBeDisabled()
  await expect(page.getByRole('heading', { name: 'Transcribing locally…' })).toBeVisible()
  const retry = page.getByRole('button', { name: 'Start recording' })
  await expect(retry).toBeEnabled({ timeout: 2_000 })
  await retry.click()
  await expect(page.getByRole('button', { name: 'Stop and transcribe' })).toBeEnabled()
})
