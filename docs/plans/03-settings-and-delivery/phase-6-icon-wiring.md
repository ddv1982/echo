# Phase 6: icon wiring

Back to [overview](overview.md).

## Goal

The same mark in the window, the dock, the tray, the app menu, and the webview, at the right size in each, under one name.

## Changes

**`src-tauri/tauri.conf.json`.** Replace the single-entry `"icon": ["icons/icon.png"]` with the full generated set. One 256x256 entry produces exactly one installed icon at `hicolor/256x256/apps/`, and nothing at 16, 22, 24, 32, or 48, so today every panel and menu is scaling a 256 px raster down. List the generated PNGs from phase 5 in ascending size.

Add an `app.trayIcon` block. It is the supported place to point at a dedicated tray asset and it is absent today, which is why the tray silently falls back to the window icon. Set `iconPath` to the 24 px tray raster. Note that `tooltip` does nothing on Linux; the GTK implementation ignores the argument and upstream documents it as unsupported. Do not add it and assume it works.

Consider `iconAsTemplate` for the tray. It asks the platform to treat the image as a monochrome mask and recolour it to the panel's foreground, which is the behaviour that makes a tray icon correct on every theme rather than on the one you tested. Verify it does something on GTK before relying on it; several `trayIcon` options are macOS-only in practice and this plan does not assume otherwise.

**`src-tauri/src/main.rs`.** The tray currently takes `app.default_window_icon()` and clones it in, falling back to no icon at all when that returns `None` (`:255-257`). Point it at the tray asset instead, and make a missing tray icon a hard error rather than a silent build-with-no-icon. A tray with no icon is invisible, and the tray is load-bearing: `on_window_event` intercepts `CloseRequested` and hides the window (`:269-274`), so once the user closes the window the tray is the only way back.

**`frontend/index.html`.** Add `<link rel="icon">` pointing at the generated favicon. There is none today; the file has a `theme-color` meta tag and no icon reference.

**`packaging/echo.desktop`.** Resolve the naming collision, which is the substantive fix in this phase. Three names are in play today:

| Path | Icon name | Desktop file |
| --- | --- | --- |
| README manual install | `echo` | `echo.desktop` |
| Tauri deb/rpm/AppImage | `echo-desktop` | `Echo.desktop` |

The bundler derives its icon filenames and the `Icon=` key from `mainBinaryName`, and the desktop filename from `productName`. So a user who installs the deb and also follows the README gets **two menu entries pointing at two different icons**. Align on the bundler's names, since those are the ones a package produces and a package is the primary distribution path from phase 2 onward. Change `packaging/echo.desktop` to `Icon=echo-desktop` and rename the file to match.

**`README.md`.** Rewrite the desktop-entry section. It currently shells out to `ffmpeg` to produce a 256x256 PNG that already exists byte-for-byte in the repo, which is the only reason `ffmpeg` appears in an apt line for a dictation app. It installs two sizes and misses every panel size. Replace it with a loop over the generated `src-tauri/icons/` set, or better, point users at the deb from phase 2 and keep the manual path as a short fallback. Apply **technical-writing**.

## Data structures

None.

## Verification

**Static.** `cargo test --workspace` plus `cargo build --release`. A wrong path in `bundle.icon` is a compile-time panic from `tauri-codegen`, so the build is a real check here. Add `desktop-file-validate packaging/echo.desktop` to the same PR if phase 1 has not already made it a CI step.

**Runtime.** The GTK tray cannot be driven by any control skill, so this is screenshots on a real desktop.

1. Install the phase 2 deb on a GNOME session. Screenshot the panel at the default scale, then at 200% scale. The bars must be countable.

   **Screenshot the panel on a dark theme and a light theme.** One background cannot prove an alpha channel. The current icon looks correct on a light panel and shows a white box on a dark one, precisely because its corners are opaque white rather than transparent. Two backgrounds is the cheapest test that distinguishes "transparent" from "happens to match".
2. Screenshot the window titlebar and the dock/overview entry.
3. Screenshot the app menu entry and confirm there is exactly **one**, which is the assertion that proves the naming collision is fixed. Install the deb *and* run the README's manual steps, then check again.
4. Load the webview and confirm the favicon appears.
5. On a session without `libayatana-appindicator3-1` installed, confirm the app still starts. The loader sits behind a `once_cell` `Lazy`, so a missing library currently surfaces as a panic rather than a graceful skip. If it panics, fix it here; an app that dies because a panel library is absent is not acceptable and phase 1 only adds the package to CI, not to every user's machine.

Attach all screenshots to the PR.
