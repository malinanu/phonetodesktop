import 'package:flutter/material.dart';

import '../app/settings.dart';
import '../app/theme.dart';
import '../core/hid.dart';
import 'touchpad_screen.dart';
import 'widgets.dart';

/// Bluetooth mode (Android): the phone acts as a keyboard, mouse and media remote the PC already
/// understands. Nothing is installed on the PC and no Wi-Fi is needed.
class BluetoothScreen extends StatefulWidget {
  const BluetoothScreen({
    super.key,
    required this.controller,
    required this.settings,
  });
  final BluetoothController controller;
  final AppSettings settings;

  @override
  State<BluetoothScreen> createState() => _BluetoothScreenState();
}

class _BluetoothScreenState extends State<BluetoothScreen> {
  bool _starting = false;
  String? _denied;

  BluetoothController get bt => widget.controller;

  Future<void> _start() async {
    setState(() {
      _starting = true;
      _denied = null;
    });
    final ok = await bt.start();
    if (!mounted) return;
    setState(() {
      _starting = false;
      if (!ok) _denied = 'Allow “Nearby devices” for Phone Remote in Android settings, then try again.';
    });
  }

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: Listenable.merge([bt, widget.settings]),
      builder: (context, _) {
        final c = context.pr;
        if (!bt.started) {
          return ListView(
            padding: const EdgeInsets.fromLTRB(20, 16, 20, 24),
            children: [
              Card2(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    const Kicker('Bluetooth'),
                    const SizedBox(height: 10),
                    const Text(
                      'Use your phone as a Bluetooth remote',
                      style: TextStyle(
                        fontSize: 26,
                        fontWeight: FontWeight.w700,
                        height: 1.1,
                      ),
                    ),
                    const SizedBox(height: 10),
                    Text(
                      'Control media, the mouse and the keyboard on any PC, Mac or Linux machine with Bluetooth. Nothing to install on the computer, and no Wi-Fi needed.',
                      style: TextStyle(color: c.dim, fontSize: 16),
                    ),
                    if (_denied != null) ...[
                      const SizedBox(height: 12),
                      Text(
                        _denied!,
                        style: TextStyle(color: c.bad, fontSize: 15),
                      ),
                    ],
                    const SizedBox(height: 16),
                    FilledButton(
                      onPressed: _starting ? null : _start,
                      child: Text(
                        _starting ? 'Starting…' : 'Turn on Bluetooth remote',
                      ),
                    ),
                  ],
                ),
              ),
            ],
          );
        }
        final s = bt.state;
        if (s.connected) return _connected(context);
        return ListView(
          padding: const EdgeInsets.fromLTRB(20, 16, 20, 24),
          children: [
            Card2(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  const Kicker('Not connected'),
                  const SizedBox(height: 8),
                  Text(
                    s.message.isEmpty ? 'Pick your computer below' : s.message,
                    style: const TextStyle(
                      fontSize: 20,
                      fontWeight: FontWeight.w700,
                    ),
                  ),
                  const SizedBox(height: 10),
                  Text(
                    'First time: on the computer open Bluetooth settings and add a device, then make this phone visible and pick it there.',
                    style: TextStyle(color: c.dim, fontSize: 15),
                  ),
                  const SizedBox(height: 14),
                  OutlinedButton.icon(
                    onPressed: bt.makeDiscoverable,
                    icon: const Icon(Icons.visibility_rounded),
                    label: const Text('Make this phone visible'),
                  ),
                ],
              ),
            ),
            const SizedBox(height: 20),
            Row(
              children: [
                const Expanded(child: Kicker('Paired computers')),
                IconButton(
                  tooltip: 'Refresh',
                  onPressed: bt.refreshPcs,
                  icon: const Icon(Icons.refresh_rounded),
                ),
              ],
            ),
            if (bt.pcs.isEmpty)
              Padding(
                padding: const EdgeInsets.symmetric(vertical: 12),
                child: Text(
                  'None yet. Pair this phone from the computer’s Bluetooth settings.',
                  style: TextStyle(color: c.dim, fontSize: 15),
                ),
              ),
            for (final pc in bt.pcs)
              Card2(
                padding: EdgeInsets.zero,
                child: Material(
                  type: MaterialType.transparency,
                  child: ListTile(
                    leading: Icon(
                      pc.isComputer
                          ? Icons.computer_rounded
                          : Icons.bluetooth_rounded,
                    ),
                    title: Text(pc.name),
                    subtitle: Text(pc.isComputer ? 'Computer' : 'Other device'),
                    onTap: () => bt.connectTo(pc),
                  ),
                ),
              ),
            const SizedBox(height: 16),
            Text(
              'If the mouse or keyboard do not work after an app update, remove this phone from the computer’s Bluetooth list and pair it again.',
              style: TextStyle(color: c.dim, fontSize: 13),
            ),
          ],
        );
      },
    );
  }

  Widget _connected(BuildContext context) {
    final c = context.pr;
    void tap(VoidCallback f) {
      buzz(widget.settings);
      f();
    }

    return Column(
      children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(20, 8, 20, 4),
          child: Row(
            mainAxisAlignment: MainAxisAlignment.spaceEvenly,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              _fit(
                PadButton(
                  icon: Icons.skip_previous_rounded,
                  label: 'Previous',
                  size: 56,
                  onPressed: () => tap(bt.prev),
                ),
              ),
              _fit(
                PadButton(
                  icon: Icons.volume_down_rounded,
                  label: 'Quieter',
                  size: 56,
                  onPressed: () => tap(() => bt.volume(-1)),
                ),
              ),
              _fit(
                PadButton(
                  icon: Icons.play_arrow_rounded,
                  label: 'Play/Pause',
                  filled: true,
                  size: 72,
                  onPressed: () => tap(bt.playPause),
                ),
              ),
              _fit(
                PadButton(
                  icon: Icons.volume_up_rounded,
                  label: 'Louder',
                  size: 56,
                  onPressed: () => tap(() => bt.volume(1)),
                ),
              ),
              _fit(
                PadButton(
                  icon: Icons.skip_next_rounded,
                  label: 'Next',
                  size: 56,
                  onPressed: () => tap(bt.next),
                ),
              ),
            ],
          ),
        ),
        if (bt.notice != null)
          Padding(
            padding: const EdgeInsets.symmetric(horizontal: 20),
            child: Text(
              bt.notice!,
              style: TextStyle(color: c.bad, fontSize: 14),
            ),
          ),
        Expanded(
          child: TouchpadScreen(link: bt, settings: widget.settings),
        ),
      ],
    );
  }

  Widget _fit(Widget w) => Flexible(
    child: FittedBox(fit: BoxFit.scaleDown, child: w),
  );
}
