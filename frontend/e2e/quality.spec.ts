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

const transcription = `
  previewDesktopApi.seedPreviewStatus({ phase: 'Transcribing', recordingSessionId: 'quality-session', recordingRevision: 4 });
  window.addEventListener('quality-injecting', () => previewDesktopApi.seedPreviewStatus({ phase: 'Injecting', recordingRevision: 5 }));
`

test('Home cancellation has pending feedback and permits a new recording', async ({ page }, testInfo) => {
  await configurePreview(page, `${transcription}
    let requests = 0;
    desktopApi.cancelTranscription = async sessionId => {
      document.documentElement.dataset.cancelRequests = String(++requests);
      await new Promise(resolve => window.addEventListener('quality-finish-cancel', resolve, { once: true }));
      previewDesktopApi.seedPreviewStatus({ phase: 'Failed', lastError: 'Transcription canceled', recordingRevision: 5 });
      return { phase: 'Transcribing', sessionId, revision: 4, captureStopRequested: false };
    };
  `)
  await page.goto('/')
  const cancel = page.getByRole('button', { name: 'Cancel transcription', exact: true })
  await expect(cancel).toBeVisible()
  await cancel.focus()
  await expect(cancel).toBeFocused()
  await page.keyboard.press('Enter')
  await expect(page.getByRole('button', { name: /Cancel/ })).toBeDisabled()
  await expect(page.locator('html')).toHaveAttribute('data-cancel-requests', '1')
  await page.screenshot({ path: testInfo.outputPath('cancel-pending.png'), fullPage: true })
  await page.evaluate(() => window.dispatchEvent(new Event('quality-finish-cancel')))
  await expect(page.getByRole('heading', { name: /cancel/i })).toBeVisible()
  await page.screenshot({ path: testInfo.outputPath('cancel-complete.png'), fullPage: true })
  await page.getByRole('button', { name: /record.*again/i }).click()
  await expect(page.getByRole('heading', { name: 'Listening…' })).toBeVisible()
})

test('Home removes cancellation when insertion starts', async ({ page }, testInfo) => {
  await configurePreview(page, transcription)
  await page.goto('/')
  await expect(page.getByRole('button', { name: 'Cancel transcription', exact: true })).toBeVisible()
  await page.evaluate(() => window.dispatchEvent(new Event('quality-injecting')))
  await expect(page.getByRole('heading', { name: 'Inserting transcript…' })).toBeVisible()
  await expect(page.getByRole('button', { name: /Cancel/ })).toHaveCount(0)
  await page.screenshot({ path: testInfo.outputPath('inserting.png'), fullPage: true })
})

test('Home copies displayed text and reports clipboard errors', async ({ page }, testInfo) => {
  await configurePreview(page, `
    previewDesktopApi.seedPreviewStatus({ lastTranscript: 'Recovery transcript', lastHistoryId: 'recovery' });
    desktopApi.getHistory = async () => [{ id: 'recovery', text: 'Recovery transcript', raw: 'Recovery transcript', engine: 'fake', startedAt: 1787310400, inferMs: 1, injection: 'ClipboardOnly' }];
    let fail = false;
    window.addEventListener('quality-copy-error', () => { fail = true; });
    desktopApi.copyText = async text => {
      if (fail) throw new Error('Clipboard is unavailable. Try copying again.');
      document.documentElement.dataset.copiedText = text;
    };
  `)
  await page.goto('/')
  await page.getByRole('button', { name: /Copy.*transcript/i }).click()
  await expect(page.locator('html')).toHaveAttribute('data-copied-text', 'Recovery transcript')
  await page.screenshot({ path: testInfo.outputPath('copy-success.png'), fullPage: true })
  await page.evaluate(() => window.dispatchEvent(new Event('quality-copy-error')))
  await page.getByRole('button', { name: /Cop.*transcript/i }).click()
  await expect(page.getByRole('alert')).toContainText('Clipboard is unavailable')
  await page.screenshot({ path: testInfo.outputPath('copy-error.png'), fullPage: true })
  await page.getByRole('button', { name: 'History', exact: true }).click()
  await expect(page.getByText(/clipboard/i).filter({ hasNotText: 'unavailable' }).first()).toBeVisible()
  await page.screenshot({ path: testInfo.outputPath('history-recovery.png'), fullPage: true })
})

for (const surface of ['Home', 'Settings']) {
  test(`${surface} rejects a microphone test completed after selection changes`, async ({ page }, testInfo) => {
    await configurePreview(page, `
      const readiness = desktopApi.getReadiness.bind(desktopApi);
      desktopApi.getReadiness = async () => ({ ...await readiness(), microphoneReady: false, firstRunComplete: false });
      const testInput = desktopApi.testInputDevice.bind(desktopApi);
      desktopApi.testInputDevice = async id => {
        const result = await testInput(id);
        await new Promise(resolve => window.addEventListener('quality-finish-test', resolve, { once: true }));
        return result;
      };
    `)
    await page.goto('/')
    if (surface === 'Settings') await page.getByRole('button', { name: 'Settings', exact: true }).click()
    const testSelected = page.getByRole('button', { name: 'Test selected', exact: true })
    await expect(testSelected).toBeVisible()
    await testSelected.click()
    await expect(testSelected).toBeDisabled()
    await page.getByRole('radio', { name: /Jabra Elite/ }).check()
    await page.evaluate(() => window.dispatchEvent(new Event('quality-finish-test')))
    await expect(testSelected).toBeEnabled()
    await expect(page.getByText(/Input heard on/)).toHaveCount(0)
    await expect(page.getByRole('radio', { name: /Jabra Elite/ })).toBeChecked()
    await page.screenshot({ path: testInfo.outputPath(`${surface.toLowerCase()}-microphone.png`), fullPage: true })
  })
}

test('Recovery controls remain visible and keyboard accessible at narrow width', async ({ page }, testInfo) => {
  await page.setViewportSize({ width: 390, height: 844 })
  await configurePreview(page, transcription)
  await page.goto('/')
  const cancel = page.getByRole('button', { name: 'Cancel transcription', exact: true })
  await expect(cancel).toBeVisible()
  await cancel.focus()
  await expect(cancel).toBeFocused()
  const box = await cancel.boundingBox()
  expect(box).not.toBeNull()
  expect(box!.x).toBeGreaterThanOrEqual(0)
  expect(box!.x + box!.width).toBeLessThanOrEqual(390)
  await page.screenshot({ path: testInfo.outputPath('narrow-cancel.png'), fullPage: true })
  await testInfo.attach('accessibility', { body: await page.locator('body').ariaSnapshot(), contentType: 'text/plain' })
})

test('History explains unconfirmed target insertion and offers copying', async ({ page }, testInfo) => {
  await configurePreview(page, `
    previewDesktopApi.seedPreviewStatus({ phase: 'Failed', lastTranscript: 'claude code', lastHistoryId: 'native-failure' });
    desktopApi.getHistory = async () => [{ id: 'native-failure', text: 'claude code', raw: 'claude code', engine: 'fake', startedAt: 1787310400, inferMs: 0, injection: 'Failed { reason: InjectUnconfirmed }' }];
  `)
  await page.goto('/')
  await page.getByRole('button', { name: 'History', exact: true }).click()
  await expect(page.getByText('Insertion failed. Copy to paste.')).toBeVisible()
  await expect(page.getByRole('button', { name: 'Copy transcript', exact: true })).toBeEnabled()
  await page.screenshot({ path: testInfo.outputPath('unconfirmed-history.png'), fullPage: true })
})
