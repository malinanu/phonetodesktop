#!/usr/bin/env python3
"""Opens a full-screen window on the current X display and logs the input events it receives
(used by the live X11 input test, see docs/DESKTOP-TESTING.md)."""
import sys, time
from Xlib import X, XK, display

LOG = sys.argv[1]
SECS = float(sys.argv[2]) if len(sys.argv) > 2 else 8
d = display.Display()
scr = d.screen()
win = scr.root.create_window(0, 0, scr.width_in_pixels, scr.height_in_pixels, 0, scr.root_depth,
                             event_mask=X.KeyPressMask | X.ButtonPressMask | X.PointerMotionMask)
win.map()
d.sync()
win.set_input_focus(X.RevertToParent, X.CurrentTime)
d.flush()

def log(s):
    with open(LOG, "a") as f:
        f.write(s + "\n")

log("ready")
end = time.time() + SECS
last = None
while time.time() < end:
    while d.pending_events():
        e = d.next_event()
        if e.type == X.KeyPress:
            sym = d.keycode_to_keysym(e.detail, 0)
            log("key %s state=%d" % (XK.keysym_to_string(sym) or hex(sym), e.state))
        elif e.type == X.ButtonPress:
            log("button %d" % e.detail)
        elif e.type == X.MotionNotify:
            last = (e.root_x, e.root_y)
    time.sleep(0.05)
if last:
    log("pointer %d %d" % last)
log("done")
