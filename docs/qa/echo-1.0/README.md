# Echo 1.0 frontend

Echo uses a full-width workspace with labeled navigation and a text recording button. The latest transcript sits above recent history and compact usage counts. Warm light and charcoal dark themes share the same controls and spacing.

We compared a sidebar studio with a horizontal workspace at the 920 × 680 default window and 760 × 560 minimum. The horizontal layout preserves more room for transcripts and Settings. The final design uses that layout, the studio's warm palette, and a distinct recording color. Existing controllers and generated IPC types still own application state.

![Home in light mode](home-light.png)

![Home in dark mode](home-dark.png)

## Reproduce the screenshots

The browser preview uses disposable fixtures, not real audio or local transcripts. These Home screenshots represent completed shortcut verification. The capture command also saves setup, recording, History, Dictionary, and Settings at four widths in both themes.

Start the preview in one terminal:

```sh
npm run dev --prefix frontend -- --host 127.0.0.1 --port 4178
```

Run the capture in another terminal from the repository root:

```sh
node frontend/scripts/capture-design.mjs
```

Images are written to `frontend/test-results/design`. Pass a destination as the first argument to choose another directory.

## Verify interactions

```sh
./scripts/verify-settings-ux.sh
npm run build --prefix frontend
npm run lint --prefix frontend
npm run test --prefix frontend
```

The browser suite checks navigation at 390, 680, 681, 760, 920, and 1280 pixels, keyboard recording, processing controls, search, dictionary edits and long phrases, dialog focus, reduced motion, and secondary-text contrast. The existing Settings suite also checks breakpoint edges, microphone controls, theme selection, and long diagnostics.

Browser verification covers the React interface with the preview API. Native microphone capture, global shortcuts, text insertion, and Linux packaging remain covered by the repository's native tests and release workflows.
