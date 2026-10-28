# Remove the Wayland titlebar workaround

`show_main_window` toggles resizability after restoring a tray-hidden window on
Wayland so the titlebar buttons respond (tauri#11856). tao 0.36 (#1218) fixed
the cause, and Tauri 2.12 ships tao 0.37.2.

- [x] Wayland test harness drives the real GNOME session (empty workspace, PID namespace, isolated D-Bus). The installed Echo exited at the first Mutter RemoteDesktop session and was relaunched; cause unconfirmed
- [x] Remove the workaround from `show_main_window`; fmt, clippy, tests pass
- [x] On Wayland: hide to tray, reopen, titlebar maximize and close respond (screenshots)
- [ ] PR CI green
