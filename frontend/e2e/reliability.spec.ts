import { expect, test, type Page } from '@playwright/test'

test.use({ video: 'on' })

async function configurePreview(page: Page, setup: string) {
  await page.route(/\/src\/preview\.tsx(?:\?.*)?$/, async (route) => {
    const response = await route.fetch()
    const source = await response.text()
    const marker = 'configureDesktopApi(desktopApi);'
    expect(source).toContain(marker)
    await route.fulfill({ response, body: source.replace(marker, `${setup}\n${marker}`) })
  })
}

test('Home shows the current failure and recovers on retry', async ({ page }, testInfo) => {
  await configurePreview(page, `previewDesktopApi.seedPreviewStatus({
    phase: 'Failed', lastError: 'Microphone disconnected. Reconnect it and try again.'
  });`)
  await page.goto('/')
  await expect(page.getByRole('heading', { name: 'Recording did not finish' })).toBeVisible()
  await expect(page.getByRole('alert')).toContainText('Microphone disconnected')
  await page.screenshot({ path: testInfo.outputPath('home-failed.png'), fullPage: true })
  await page.getByRole('button', { name: 'Try recording again' }).click()
  await expect(page.getByRole('heading', { name: 'Listening…' })).toBeVisible()
  await expect(page.getByText('Microphone disconnected. Reconnect it and try again.')).toHaveCount(0)
  await page.screenshot({ path: testInfo.outputPath('home-recovered.png'), fullPage: true })
})

test('Dictionary retains the next draft when a delayed save completes', async ({ page }, testInfo) => {
  await configurePreview(page, `
    const originalAdd = desktopApi.addDictionaryEntry.bind(desktopApi);
    desktopApi.addDictionaryEntry = async (...args) => {
      await new Promise(resolve => window.addEventListener('echo-test-finish-save', resolve, { once: true }));
      return originalAdd(...args);
    };
  `)
  await page.goto('/')
  await page.getByRole('button', { name: 'Dictionary', exact: true }).click()
  const spoken = page.getByLabel('What Echo hears')
  const written = page.getByLabel('What Echo should write')
  await spoken.fill('first pronunciation')
  await written.fill('First entry')
  await page.getByRole('button', { name: 'Add', exact: true }).click()
  await spoken.fill('second pronunciation')
  await written.fill('Second entry')
  await page.screenshot({ path: testInfo.outputPath('dictionary-pending.png'), fullPage: true })
  await page.evaluate(() => {
    window.dispatchEvent(new Event('echo-test-finish-save'))
  })
  await expect(page.getByRole('button', { name: 'Remove First entry' })).toBeVisible()
  await expect(spoken).toHaveValue('second pronunciation')
  await expect(written).toHaveValue('Second entry')
  await expect(page.getByRole('button', { name: 'Add', exact: true })).toBeEnabled()
  await page.screenshot({ path: testInfo.outputPath('dictionary-retained.png'), fullPage: true })
})
