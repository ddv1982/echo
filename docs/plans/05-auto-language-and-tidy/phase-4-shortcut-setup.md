# Phase 4: trustworthy shortcut setup

Back to [overview](overview.md).

## Goal

The checklist's shortcut item stops being honor-system. Today it is a manual "I bound it" dismissal stored in localStorage; nothing verifies a binding exists or that it resolves to the right binary. This phase replaces it with a verified setup, and makes a researched call on in-app registration.

## Research, and the call it drives

**`org.freedesktop.portal.GlobalShortcuts`** is the Wayland-capable registration path. It landed in GNOME 48 ([release notes](https://release.gnome.org/48/developers/), [xdg-desktop-portal-gnome NEWS](https://gitlab.gnome.org/GNOME/xdg-desktop-portal-gnome/-/raw/gnome-48/NEWS)) with a rebinding bug until 48.8, and in KDE Plasma 6.3 ([KDE bug 483640](https://bugs.kde.org/show_bug.cgi?id=483640), fixed in 6.3.0). Echo's stated targets are Ubuntu and Zorin: Ubuntu 24.04 LTS ships GNOME 46 and Zorin 17 ships GNOME 43, so the portal is absent for exactly the users this app is for. **[tauri-plugin-global-shortcut](https://github.com/tauri-apps/global-hotkey/pull/162)** routes Wayland through the portal as of late 2025 and keeps an X11 default path, but the Wayland path is young: callbacks silently not firing on GNOME 48.7 ([plugins-workspace#3267](https://github.com/tauri-apps/plugins-workspace/issues/3267)) is the kind of bug that recreates "the hotkey does nothing" inside the fix for it.

**The call: registration is not shippable yet.** On the desktops Echo targets, the portal does not exist; where it exists, the plugin's Wayland path is immature. What ships instead is a verified-setup flow that works everywhere a compositor binding works, which is everywhere the toggle works today. Registration revisits when the LTSes ship GNOME 48.8+; the phase notes the trigger condition in the code.

## Changes

**`frontend/src/App.tsx`, the shortcut row.** A "Test your shortcut" flow: the user binds `echo-desktop rec --toggle` in their compositor settings as today, clicks Test, and Echo opens a ten-second listener window asking them to press the key. The press spawns the CLI, which writes the status file; the GUI's status poll sees the session start and confirms the binding works. Verification goes through the real path — compositor, binding, spawn, status file — so a stale shadowed binary or a missing binding both fail the test visibly instead of silently.

**The checklist item becomes computed.** `echo-shortcut-bound` in localStorage is replaced by a stored *verified-at* timestamp set only by a passing test. The item reads "Shortcut verified" and shows when it last passed. The manual dismissal is deleted.

**`crates/echo/src/status.rs`.** No change needed: the status file already carries state and pid, and the GUI already polls it. The test flow is a reader of what exists.

## Data structures

`shortcut_verified_at: Option<u64>` in localStorage, written only by a passing test. The listener window is a UI state with a timeout, not a new window.

## Verification

**Static.** `npm run test --prefix frontend`. Component tests: the checklist item is unverified by default; a simulated status flip during the listener window marks it verified and persists; a timeout leaves it unverified with a try-again hint.

**Runtime.** Via **control-ui** under Xvfb: open the test flow, spawn `rec --toggle` externally with the fixture, and watch the item flip to verified. Then the negative: no spawn, timeout, item stays unverified. Attach both transcripts to the PR.
