# Testing the macOS / Linux agent

Unit tests run anywhere: `cd agent && cargo test`. Cross-compile checks (no linking, no SDK needed):

```
rustup target add aarch64-apple-darwin x86_64-pc-windows-gnu
cargo check --target aarch64-apple-darwin
cargo check --target x86_64-pc-windows-gnu
```

## Live Linux tests (ignored by default)
Both need extra tools, so they are marked `#[ignore]`. They exercise the real code paths against real services.

**MPRIS (D-Bus media players)** needs `dbus-daemon` and `pip install dbus-next`:
```
cd agent
dbus-run-session -- sh -c 'python3 tests-support/fake_mpris.py /tmp/fake.log & sleep 2; cargo test live_player_roundtrip -- --ignored'
cat /tmp/fake.log     # PlayPause / Next / Previous / SetPosition as received by the fake player
```

**Mouse, keyboard, scroll, media keys (X11)** needs `Xvfb` and `pip install python-xlib`:
```
cd agent
xvfb-run -a sh -c 'python3 tests-support/x11_probe.py /tmp/x.log 8 & sleep 2; cargo test live_x11_input -- --ignored; sleep 7; cat /tmp/x.log'
```
Expected log: pointer moved by (+40,+25), buttons 1 and 3, scroll buttons 4, 4, 5, keys `h`, `i`, Control then `a` with state=4, Return, and `0x1008ff14` (XF86AudioPlay).

## Not covered by automation
- **macOS:** compiles for `aarch64-apple-darwin`, but has not been run on a Mac. Check by hand: Accessibility prompt appears, media keys, mouse/keyboard, volume (`osascript`).
- **Wayland-only desktops:** input uses X11/XWayland and may be blocked; MPRIS works everywhere there is a session bus.
