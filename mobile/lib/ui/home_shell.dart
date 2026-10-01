import 'package:flutter/material.dart';

import '../app/connection.dart';
import '../app/settings.dart';
import '../app/theme.dart';
import 'files_screen.dart';
import 'pair_screen.dart';
import 'remote_screen.dart';
import 'settings_screen.dart';
import 'touchpad_screen.dart';

class HomeShell extends StatefulWidget {
  const HomeShell({super.key, required this.link, required this.settings, this.scan = scanQr, this.pair});
  final ConnectionController link;
  final AppSettings settings;
  final Scanner scan;
  final Pairer? pair;

  @override
  State<HomeShell> createState() => _HomeShellState();
}

class _HomeShellState extends State<HomeShell> {
  int _tab = 0;

  PairScreen _pairScreen({VoidCallback? onPaired}) => widget.pair == null
      ? PairScreen(link: widget.link, onPaired: onPaired, scan: widget.scan)
      : PairScreen(link: widget.link, onPaired: onPaired, scan: widget.scan, pair: widget.pair!);

  void _openPair() {
    late final VoidCallback close;
    close = () => Navigator.of(context).maybePop();
    Navigator.of(context).push(MaterialPageRoute(builder: (_) => _pairScreen(onPaired: close)));
  }

  void _openSettings() => Navigator.of(context).push(MaterialPageRoute(builder: (_) => SettingsScreen(settings: widget.settings, link: widget.link, onAddPc: _openPair)));

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: widget.link,
      builder: (context, _) {
        final link = widget.link;
        if (link.pcs.isEmpty) return _pairScreen();
        final c = context.pr;
        final pc = link.active;
        final dot = switch (link.status) {
          LinkStatus.connected => c.good,
          LinkStatus.connecting => c.accent,
          _ => c.bad,
        };
        return Scaffold(
          appBar: AppBar(
            titleSpacing: 20,
            title: Row(children: [
              Container(width: 10, height: 10, decoration: BoxDecoration(color: dot, shape: BoxShape.circle)),
              const SizedBox(width: 10),
              Flexible(child: Text(pc?.name ?? 'Phone Remote', overflow: TextOverflow.ellipsis, style: const TextStyle(fontSize: 20, fontWeight: FontWeight.w700))),
            ]),
            actions: [
              if (link.pcs.length > 1)
                PopupMenuButton<String>(
                  tooltip: 'Switch PC',
                  icon: const Icon(Icons.computer_rounded),
                  onSelected: link.select,
                  itemBuilder: (_) => [for (final p in link.pcs) PopupMenuItem(value: p.id, child: Text(p.name))],
                ),
              IconButton(tooltip: 'Settings', icon: const Icon(Icons.settings_rounded), onPressed: _openSettings),
            ],
          ),
          body: Column(children: [
            _Banner(link: link, onPairAgain: _openPair),
            Expanded(
              child: IndexedStack(index: _tab, children: [
                RemoteScreen(link: link, settings: widget.settings),
                TouchpadScreen(link: link, settings: widget.settings),
                FilesScreen(settings: widget.settings, onOpenSettings: _openSettings),
              ]),
            ),
          ]),
          bottomNavigationBar: NavigationBar(
            selectedIndex: _tab,
            onDestinationSelected: (i) => setState(() => _tab = i),
            destinations: const [
              NavigationDestination(icon: Icon(Icons.play_circle_outline_rounded), selectedIcon: Icon(Icons.play_circle_rounded), label: 'Remote'),
              NavigationDestination(icon: Icon(Icons.touch_app_outlined), selectedIcon: Icon(Icons.touch_app_rounded), label: 'Touchpad'),
              NavigationDestination(icon: Icon(Icons.upload_file_outlined), selectedIcon: Icon(Icons.upload_file_rounded), label: 'Files'),
            ],
          ),
        );
      },
    );
  }
}

/// Says what is wrong with the connection, and what to do about it.
class _Banner extends StatelessWidget {
  const _Banner({required this.link, required this.onPairAgain});
  final ConnectionController link;
  final VoidCallback onPairAgain;

  @override
  Widget build(BuildContext context) {
    final c = context.pr;
    final name = link.active?.name ?? 'the PC';
    final (String? message, List<Widget> actions) = switch (link.status) {
      LinkStatus.connected || LinkStatus.idle => (null, const <Widget>[]),
      LinkStatus.connecting => ('Connecting to $name…', const <Widget>[]),
      LinkStatus.offline => (
          'Can’t reach $name. Same Wi-Fi? Retrying…',
          [TextButton(onPressed: link.reconnectNow, child: const Text('Try again'))],
        ),
      LinkStatus.revoked => (
          '$name removed this phone.',
          [TextButton(onPressed: onPairAgain, child: const Text('Pair again')), TextButton(onPressed: () => link.forget(link.active!.id), child: const Text('Forget'))],
        ),
      LinkStatus.keyChanged => (
          '$name now has a different security key than when you paired (it may have been reinstalled). Pair again only if you expect that.',
          [TextButton(onPressed: onPairAgain, child: const Text('Pair again')), TextButton(onPressed: () => link.forget(link.active!.id), child: const Text('Forget'))],
        ),
      LinkStatus.insecure => ('$name needs the newest Phone Remote to connect securely. Update it on the PC.', const <Widget>[]),
    };
    if (message == null) return const SizedBox.shrink();
    final bad = link.status != LinkStatus.connecting;
    return Semantics(
      liveRegion: true,
      child: Container(
        width: double.infinity,
        margin: const EdgeInsets.fromLTRB(16, 4, 16, 8),
        padding: const EdgeInsets.fromLTRB(16, 12, 8, 8),
        decoration: BoxDecoration(color: (bad ? c.bad : c.accent).withValues(alpha: .12), borderRadius: BorderRadius.circular(14), border: Border.all(color: (bad ? c.bad : c.accent).withValues(alpha: .5))),
        child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
          Text(message, style: TextStyle(color: c.ink, fontSize: 15, height: 1.3)),
          if (actions.isNotEmpty) Wrap(children: actions),
        ]),
      ),
    );
  }
}
