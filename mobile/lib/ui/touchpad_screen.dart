import 'dart:async';

import 'package:flutter/material.dart';

import '../app/input_sink.dart';
import '../app/settings.dart';
import '../app/theme.dart';
import 'widgets.dart';

/// Mouse and keyboard for the PC. One finger moves, a tap clicks, two fingers scroll, a two-finger tap right-clicks.
class TouchpadScreen extends StatefulWidget {
  const TouchpadScreen({super.key, required this.link, required this.settings});
  final InputSink link;
  final AppSettings settings;

  @override
  State<TouchpadScreen> createState() => _TouchpadScreenState();
}

class _TouchpadScreenState extends State<TouchpadScreen> {
  final Map<int, Offset> _down = {}; // pointer -> last position
  final Map<int, Offset> _start = {};
  final Map<int, Duration> _lastTime = {};
  DateTime _gestureStart = DateTime.now();
  double _travel = 0; // how far fingers moved in this gesture
  int _maxFingers = 0;
  double _mx = 0, _my = 0, _sx = 0, _sy = 0; // movement waiting to be sent
  Timer? _flush;
  final _text = TextEditingController();
  final _typed = FocusNode();

  InputSink get link => widget.link;
  AppSettings get settings => widget.settings;

  @override
  void dispose() {
    _flush?.cancel();
    _text.dispose();
    _typed.dispose();
    super.dispose();
  }

  void _schedule() => _flush ??= Timer(const Duration(milliseconds: 16), _send);

  void _send() {
    _flush = null;
    final dx = _mx.truncate(), dy = _my.truncate();
    if (dx != 0 || dy != 0) {
      link.mouseMove(dx, dy);
      _mx -= dx;
      _my -= dy;
    }
    final wx = _sx.truncate(), wy = _sy.truncate();
    if (wx != 0 || wy != 0) {
      link.scroll(wx, wy);
      _sx -= wx;
      _sy -= wy;
    }
  }

  void _onDown(PointerDownEvent e) {
    if (_down.isEmpty) {
      _gestureStart = DateTime.now();
      _travel = 0;
      _maxFingers = 0;
    }
    _down[e.pointer] = e.localPosition;
    _start[e.pointer] = e.localPosition;
    _lastTime[e.pointer] = e.timeStamp;
    if (_down.length > _maxFingers) _maxFingers = _down.length;
  }

  void _onMove(PointerMoveEvent e) {
    final last = _down[e.pointer];
    if (last == null) return;
    final d = e.localPosition - last;
    final dt = (e.timeStamp - (_lastTime[e.pointer] ?? e.timeStamp)).inMilliseconds.clamp(1, 1000);
    _down[e.pointer] = e.localPosition;
    _lastTime[e.pointer] = e.timeStamp;
    _travel += d.distance;
    if (_down.length >= 2) {
      // Two fingers scroll, like the web touchpad: each finger reports its own move, so use the average.
      // Wheel units: 120 is one notch. Natural = the content follows the fingers.
      final dir = settings.scrollNatural ? 1.0 : -1.0;
      _sx += d.dx / 2 * 4 * dir;
      _sy += d.dy / 2 * 4 * dir;
    } else {
      // A gentle acceleration, scaled by the speed setting (same curve as the web touchpad).
      final speed = d.distance / dt; // px per ms
      final gain = (1.1 + (speed * 1.1).clamp(0.0, 1.9)) * settings.padSpeed;
      _mx += d.dx * gain;
      _my += d.dy * gain;
    }
    _schedule();
  }

  void _onUp(PointerEvent e) {
    final wasLast = _down.length == 1;
    _down.remove(e.pointer);
    _lastTime.remove(e.pointer);
    if (!wasLast) return;
    _start.clear();
    final quick = DateTime.now().difference(_gestureStart) < const Duration(milliseconds: 300);
    if (quick && _travel < 12 && e is PointerUpEvent) {
      if (_maxFingers >= 2) {
        buzz(settings);
        link.mouseButton('right', 'click');
      } else if (settings.tapToClick) {
        buzz(settings);
        link.mouseButton('left', 'click');
      }
    }
  }

  void _sendTyped(String s) {
    if (s.isEmpty) return;
    link.typeText(s);
    _text.clear();
  }

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: Listenable.merge([link, settings]),
      builder: (context, _) {
        final c = context.pr;
        final online = link.inputConnected;
        if (online && !link.inputAllowed) {
          return Padding(
            padding: const EdgeInsets.all(28),
            child: Center(
              child: Column(mainAxisSize: MainAxisSize.min, children: [
                Icon(Icons.mouse_outlined, size: 48, color: c.dim),
                const SizedBox(height: 16),
                const Text('Mouse and keyboard are off for this phone', textAlign: TextAlign.center, style: TextStyle(fontSize: 22, fontWeight: FontWeight.w700)),
                const SizedBox(height: 10),
                Text('On the PC, open Phone Remote, go to Phones and turn on “Mouse & keyboard” next to this phone.', textAlign: TextAlign.center, style: TextStyle(color: c.dim, fontSize: 16)),
              ]),
            ),
          );
        }
        return Padding(
          padding: const EdgeInsets.fromLTRB(20, 8, 20, 16),
          child: Column(children: [
            Expanded(
              child: Listener(
                onPointerDown: online ? _onDown : null,
                onPointerMove: online ? _onMove : null,
                onPointerUp: online ? _onUp : null,
                onPointerCancel: online ? _onUp : null,
                behavior: HitTestBehavior.opaque,
                child: Semantics(
                  label: 'Touchpad. Drag to move the cursor, tap to click, two fingers to scroll.',
                  child: Container(
                    width: double.infinity,
                    decoration: BoxDecoration(color: c.surface, borderRadius: BorderRadius.circular(24), border: Border.all(color: c.line)),
                    alignment: Alignment.center,
                    child: Padding(
                      padding: const EdgeInsets.all(24),
                      child: Text(online ? 'Drag to move the cursor\nTap to click · two fingers: tap = right click, drag = scroll' : 'Not connected', textAlign: TextAlign.center, style: TextStyle(color: c.dim, fontSize: 15, height: 1.4)),
                    ),
                  ),
                ),
              ),
            ),
            const SizedBox(height: 12),
            Row(children: [
              Expanded(child: OutlinedButton(onPressed: online ? () { buzz(settings); link.mouseButton('left', 'click'); } : null, child: const Text('Left'))),
              const SizedBox(width: 10),
              Expanded(child: OutlinedButton(onPressed: online ? () { buzz(settings); link.mouseButton('right', 'click'); } : null, child: const Text('Right'))),
            ]),
            const SizedBox(height: 12),
            TextField(
              controller: _text,
              focusNode: _typed,
              enabled: online,
              textInputAction: TextInputAction.send,
              onSubmitted: (s) {
                _sendTyped(s);
                link.pressKey('enter');
                _typed.requestFocus();
              },
              decoration: InputDecoration(
                hintText: 'Type on the PC',
                suffixIcon: IconButton(tooltip: 'Send', icon: const Icon(Icons.send_rounded), onPressed: online ? () => _sendTyped(_text.text) : null),
              ),
            ),
            const SizedBox(height: 10),
            SizedBox(
              height: 44,
              child: ListView(scrollDirection: Axis.horizontal, children: [
                for (final k in const [('Esc', 'esc'), ('Tab', 'tab'), ('⌫', 'backspace'), ('Enter', 'enter'), ('←', 'left'), ('↑', 'up'), ('↓', 'down'), ('→', 'right'), ('Win', 'win')])
                  Padding(
                    padding: const EdgeInsets.only(right: 8),
                    child: OutlinedButton(
                      style: OutlinedButton.styleFrom(minimumSize: const Size(52, 44), padding: const EdgeInsets.symmetric(horizontal: 14)),
                      onPressed: online ? () { buzz(settings); link.pressKey(k.$2); } : null,
                      child: Text(k.$1),
                    ),
                  ),
                for (final k in const [('Copy', 'c'), ('Paste', 'v'), ('Undo', 'z')])
                  Padding(
                    padding: const EdgeInsets.only(right: 8),
                    child: OutlinedButton(
                      style: OutlinedButton.styleFrom(minimumSize: const Size(52, 44), padding: const EdgeInsets.symmetric(horizontal: 14)),
                      onPressed: online ? () { buzz(settings); link.pressKey(k.$2, const ['ctrl']); } : null,
                      child: Text(k.$1),
                    ),
                  ),
              ]),
            ),
          ]),
        );
      },
    );
  }
}
