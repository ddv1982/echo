# Echo QA

Use this directory for current manual product verification and reproducible
bug reports. `qa-config.json` configures a new review run. Put confirmed bugs
under `bug-reports/`.

The frozen 2026-08-25 acceleration runs and generated HTML reports are in the
[historical evidence archive](../history/evidence-2026-08-30.md). Echo no longer
uses a separate acceleration admission gate. The Advanced readout reports the
device that ran and explains a GPU-to-CPU fallback.

## Physical acceptance

Signed off 2026-09-08 on this Zorin GNOME Wayland host against the installed
`echo` 1.0.0 package (`/usr/bin/echo-desktop` running).

| Gate | Evidence |
| --- | --- |
| Shortcut | GNOME custom keybinding `echo`: `<Super><Alt>space` → `/usr/bin/echo-desktop rec --toggle` |
| Microphone | Saved PipeWire device `BRIO Ultra HD Webcam Analoog stereo`; capture hardware also includes sof-hda-dsp DMIC and Samson Meteor |
| Insertion | Live history: 164 rows, all `Typed` / `Ydotool`, 47 nonempty, 2026-09-01 through 2026-09-07 |
| Routine use | Same history file on the daily-driver account over that week |

Synthetic CI still does not replace this host observation. Empty trailing
rows are silence or cancelled takes; they do not undo the nonempty typed
inserts.
