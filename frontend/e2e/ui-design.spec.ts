import { mkdir } from 'node:fs/promises'
import { env } from 'node:process'
import { resolve } from 'node:path'
import { expect, test, type Page, type TestInfo } from '@playwright/test'

const viewports = [
  { width: 1280, height: 900 },
  { width: 920, height: 680 },
  { width: 760, height: 560 },
  { width: 390, height: 844 },
]

async function capture(page: Page, info: TestInfo, name: string) {
  const directory = env.ECHO_UI_EVIDENCE_DIR
    ? resolve(env.ECHO_UI_EVIDENCE_DIR)
    : resolve('../.audit/ui-design/after')
  await mkdir(directory, { recursive: true })
  await page.screenshot({ path: resolve(directory, `${name}.png`), fullPage: true })
  await info.attach(name, { path: resolve(directory, `${name}.png`), contentType: 'image/png' })
}

async function expectFits(page: Page) {
  expect(await page.evaluate(() => {
    const fits = (node: Element) => node.scrollWidth <= node.clientWidth
    const main = document.querySelector('main.main-content')
    return fits(document.documentElement) && main !== null && fits(main)
  })).toBe(true)
}

for (const theme of ['light', 'dark']) {
  test(`UI design evidence across views and sizes in ${theme}`, async ({ page }, info) => {
    test.setTimeout(90_000)
    const errors: string[] = []
    page.on('pageerror', (error) => errors.push(error.message))
    await page.addInitScript((mode) => localStorage.setItem('echo-theme', mode), theme)
    for (const viewport of viewports) {
      await page.setViewportSize(viewport)
      await page.goto('/')
      await expect(page.getByRole('heading', { name: 'Ready when you are' })).toBeVisible()
      await expect(page.locator('html')).toHaveAttribute('data-theme', theme)
      for (const view of ['Home', 'History', 'Dictionary', 'Settings']) {
        await page.getByRole('button', { name: view, exact: true }).click()
        await expect(page.locator('main .view-stack')).toBeVisible()
        await page.evaluate(() => document.fonts.ready)
        await expectFits(page)
        await expect(page.locator('html')).toHaveAttribute('data-theme', theme)
        await capture(page, info, `${theme}-${viewport.width}-${view.toLowerCase()}`)
      }
      await page.getByRole('button', { name: 'Home', exact: true }).click()
      await page.getByRole('button', { name: 'Start recording', exact: true }).click()
      await expect(page.getByRole('heading', { name: 'Listening…' })).toBeVisible()
      await expect(page.locator('html')).toHaveAttribute('data-theme', theme)
      await capture(page, info, `${theme}-${viewport.width}-recording`)
      await page.getByRole('button', { name: 'Stop and transcribe', exact: true }).click()
      await expect(page.getByRole('heading', { name: 'Ready when you are' })).toBeVisible()
    }
    expect(errors).toEqual([])
  })
}

test('UI design navigation remains recognizable and keyboard usable at compact widths', async ({ page }) => {
  for (const width of [920, 760, 390, 320]) {
    await page.setViewportSize({ width, height: 680 })
    await page.goto('/')
    for (const name of ['Home', 'History', 'Dictionary', 'Settings']) {
      const button = page.getByRole('button', { name, exact: true })
      await expect(button.locator('span')).toBeVisible()
      await button.focus()
      await expect(button).toBeFocused()
      await page.keyboard.press('Enter')
      await expect(button).toHaveAttribute('aria-current', 'page')
    }
    await expectFits(page)
  }
})

test('UI design recording respects reduced motion', async ({ page }) => {
  await page.emulateMedia({ reducedMotion: 'reduce' })
  await page.goto('/')
  await page.getByRole('button', { name: 'Start recording', exact: true }).click()
  await expect(page.getByRole('heading', { name: 'Listening…' })).toBeVisible()
  const durations = await page.locator('.record-ring, .status-dot').evaluateAll((nodes) =>
    nodes.map((node) => Number.parseFloat(getComputedStyle(node).animationDuration)),
  )
  expect(durations.every((duration) => duration <= 0.001)).toBe(true)
  await page.getByRole('button', { name: 'Stop and transcribe', exact: true }).click()
})

// ui-polish.R13-03: each theme gets an isolated context and one init script.
for (const theme of ['light', 'dark']) {
  test(`UI design semantic colors and focus are readable in ${theme}`, async ({ page }, info) => {
    await page.addInitScript((mode) => localStorage.setItem('echo-theme', mode), theme)
    await page.goto('/')
    await expect(page.getByRole('heading', { name: 'Ready when you are' })).toBeVisible()
    await expect(page.locator('html')).toHaveAttribute('data-theme', theme)
    const contrasts = await page.evaluate(() => {
      const styles = getComputedStyle(document.documentElement)
      const canvas = document.createElement('canvas')
      canvas.width = canvas.height = 1
      const context = canvas.getContext('2d')!
      function luminance(token: string) {
        context.fillStyle = `hsl(${styles.getPropertyValue(token)})`
        context.fillRect(0, 0, 1, 1)
        const pixel = context.getImageData(0, 0, 1, 1).data
        const linear = (value: number) => {
          const channel = value / 255
          return channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4
        }
        return linear(pixel[0]!) * 0.2126 + linear(pixel[1]!) * 0.7152 + linear(pixel[2]!) * 0.0722
      }
      function ratio(foreground: string, background: string) {
        const first = luminance(foreground)
        const second = luminance(background)
        return (Math.max(first, second) + 0.05) / (Math.min(first, second) + 0.05)
      }
      const surfaces = ['--surface-app', '--surface-sidebar', '--surface-card', '--surface-raised', '--surface-hover']
      return {
        text: ['--foreground', '--text-secondary', '--text-tertiary'].flatMap((foreground) =>
          surfaces.map((background) => ({ foreground, background, ratio: ratio(foreground, background) })),
        ),
        controls: ['--control-border', '--focus'].flatMap((foreground) =>
          surfaces.map((background) => ({ foreground, background, ratio: ratio(foreground, background) })),
        ),
        recording: ratio('--on-recording', '--recording'),
      }
    })
    await info.attach(`${theme}-contrast`, { body: JSON.stringify(contrasts, null, 2), contentType: 'application/json' })
    for (const pair of contrasts.text) expect(pair.ratio, `${theme}: ${pair.foreground} on ${pair.background}`).toBeGreaterThanOrEqual(4.5)
    for (const pair of contrasts.controls) expect(pair.ratio, `${theme}: ${pair.foreground} on ${pair.background}`).toBeGreaterThanOrEqual(3)
    expect(contrasts.recording).toBeGreaterThanOrEqual(4.5)
    const navigation = page.getByRole('button', { name: 'History', exact: true })
    await navigation.focus()
    await expect(navigation).toBeFocused()
    expect(await navigation.evaluate((node) => getComputedStyle(node).outlineStyle)).not.toBe('none')
    await capture(page, info, `${theme}-keyboard-focus`)
  })
}

// ui-polish.R13-01: verify rendered controls, not the grid implementation.
for (const viewport of viewports.filter(({ width }) => width === 760 || width === 920)) {
  test(`UI design compact Dictionary has equal fields and three controls in one row at ${viewport.width}px`, async ({ page }, info) => {
    await page.setViewportSize(viewport)
    await page.goto('/')
    await page.getByRole('button', { name: 'Dictionary', exact: true }).click()
    const form = page.locator('.dictionary-form')
    const spoken = form.getByRole('textbox', { name: 'What Echo hears', exact: true })
    const written = form.getByRole('textbox', { name: 'What Echo should write', exact: true })
    const add = form.getByRole('button', { name: 'Add', exact: true })
    await expect(form.getByRole('textbox')).toHaveCount(2)
    await expect(form.getByRole('button')).toHaveCount(1)
    for (const control of [spoken, written, add]) await expect(control).toBeVisible()
    await page.evaluate(() => document.fonts.ready)
    await capture(page, info, `compact-${viewport.width}-dictionary-controls`)
    const boxes = await Promise.all([spoken, written, add].map((control) => control.boundingBox()))
    const [spokenBox, writtenBox, addBox] = boxes
    if (!spokenBox || !writtenBox || !addBox) throw new Error('Dictionary controls must have rendered bounds')
    expect(Math.abs(spokenBox.width - writtenBox.width), 'Dictionary fields should have equal widths').toBeLessThanOrEqual(1)
    expect(spokenBox.x + spokenBox.width).toBeLessThanOrEqual(writtenBox.x)
    expect(writtenBox.x + writtenBox.width).toBeLessThanOrEqual(addBox.x)
    for (const box of [writtenBox, addBox]) {
      expect(Math.abs(spokenBox.y + spokenBox.height - box.y - box.height), 'Dictionary controls should share a row baseline').toBeLessThanOrEqual(1)
    }
    await expectFits(page)
  })
}

test('UI design Home Copy transcript uses the compact non-primary button height', async ({ page }, info) => {
  // ui-polish.R13-02: the checklist has a separate height override and is not the subject.
  await page.setViewportSize({ width: 920, height: 680 })
  await page.goto('/')
  await expect(page.getByRole('heading', { name: 'Ready when you are' })).toBeVisible()
  const copy = page.locator('.last-transcript').getByRole('button', { name: 'Copy transcript', exact: true })
  await expect(copy).toBeVisible()
  await expect(copy).toHaveClass(/\bcompact-button\b/)
  expect(await copy.evaluate((node) => node.closest('.checklist-item') !== null)).toBe(false)
  await page.evaluate(() => document.fonts.ready)
  await capture(page, info, 'compact-920-home-copy-transcript')
  const height = await copy.evaluate((node) => ({
    minHeight: Number.parseFloat(getComputedStyle(node).minHeight),
    rootFontSize: Number.parseFloat(getComputedStyle(document.documentElement).fontSize),
  }))
  expect(Math.abs(height.minHeight - 2.55 * height.rootFontSize), 'Compact Copy transcript min-height should be 2.55rem').toBeLessThanOrEqual(0.05)
})

test('UI design empty and processing states keep their actions at narrow width', async ({ page }, info) => {
  await page.setViewportSize({ width: 390, height: 844 })
  await page.route(/\/src\/preview\.tsx(?:\?.*)?$/, async (route) => {
    const response = await route.fetch()
    const source = await response.text()
    const marker = 'configureDesktopApi(desktopApi);'
    expect(source).toContain(marker)
    await route.fulfill({ response, body: source.replace(marker, `
      desktopApi.getHistory = async () => [];
      desktopApi.getDictionary = async () => [];
      previewDesktopApi.seedPreviewStatus({ lastTranscript: '', phase: 'Transcribing', recordingSessionId: 'design-session', recordingRevision: 4 });
      window.addEventListener('design-fail', () => previewDesktopApi.seedPreviewStatus({ phase: 'Failed', lastError: 'Microphone unavailable. Select another input in Settings.', recordingRevision: 5 }));
      ${marker}
    `) })
  })
  await page.goto('/')
  await expect(page.getByRole('button', { name: 'Cancel transcription', exact: true })).toBeVisible()
  await expectFits(page)
  await capture(page, info, 'narrow-processing-empty')
  await page.evaluate(() => window.dispatchEvent(new Event('design-fail')))
  await expect(page.getByRole('alert')).toContainText('Microphone unavailable')
  await expect(page.getByRole('button', { name: 'Try recording again' })).toBeVisible()
  await capture(page, info, 'narrow-error')
  for (const view of ['History', 'Dictionary']) {
    await page.getByRole('button', { name: view, exact: true }).click()
    await expectFits(page)
    await capture(page, info, `narrow-empty-${view.toLowerCase()}`)
  }
})
