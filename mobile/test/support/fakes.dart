import 'dart:io';
import 'dart:ui' as ui;

import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:phoneremote/app/connection.dart';
import 'package:phoneremote/app/pc_store.dart';
import 'package:phoneremote/app/theme.dart';
import 'package:phoneremote/core/identity.dart';
import 'package:phoneremote/core/protocol.dart';
import 'package:phoneremote/core/session.dart';

/// A controller whose state is set by the test; it records the commands the screens send.
class FakeLink extends ConnectionController {
  FakeLink({LinkStatus status = LinkStatus.connected, AgentState? state, bool input = true, List<PcRecord>? pcs})
      : _pcs = pcs ?? [const PcRecord(id: 'pc-1', name: 'Living room PC', host: '192.168.1.20', port: 8765, fingerprint: 'fp')],
        super(pcStore: MemoryPcStore(), identityStore: MemoryIdentityStore()) {
    _status = status;
    _state = state;
    _input = input;
  }

  LinkStatus _status = LinkStatus.connected;
  AgentState? _state;
  bool _input = true;
  List<PcRecord> _pcs;
  final List<String> calls = [];

  void update({LinkStatus? status, AgentState? state, bool? input}) {
    _status = status ?? _status;
    _state = state ?? _state;
    _input = input ?? _input;
    notifyListeners();
  }

  @override
  LinkStatus get status => _status;
  @override
  AgentState? get state => _state;
  @override
  bool get inputAllowed => _input;
  @override
  List<PcRecord> get pcs => List.unmodifiable(_pcs);
  @override
  PcRecord? get active => _pcs.isEmpty ? null : _pcs.first;
  @override
  int estimatedPosMs(PlayerInfo p) => p.posMs;

  @override
  Future<void> addPairedPc(PcRecord pc) async {
    calls.add('addPaired ${pc.id}');
    _pcs = [pc];
    _status = LinkStatus.connected;
    notifyListeners();
  }

  @override
  Future<void> forget(String id) async {
    calls.add('forget $id');
    _pcs = _pcs.where((p) => p.id != id).toList();
    notifyListeners();
  }

  @override
  void reconnectNow() => calls.add('reconnect');
  @override
  Future<void> select(String id) async => calls.add('select $id');
  @override
  void playPause() => calls.add('playPause');
  @override
  void next() => calls.add('next');
  @override
  void prev() => calls.add('prev');
  @override
  void seekRel(int seconds) => calls.add('seekRel $seconds');
  @override
  void seekAbs(int posMs) => calls.add('seekAbs $posMs');
  @override
  void volume(int steps) => calls.add('volume $steps');
  @override
  void volumeSet(int level) => calls.add('volumeSet $level');
  @override
  void mute() => calls.add('mute');
  @override
  void selectPlayer(String id) => calls.add('selectPlayer $id');
  @override
  void mouseMove(int dx, int dy) => calls.add('move $dx $dy');
  @override
  void mouseButton(String button, String action) => calls.add('button $button $action');
  @override
  void scroll(int dx, int dy) => calls.add('scroll $dx $dy');
  @override
  void typeText(String s) => calls.add('text $s');
  @override
  void pressKey(String name, [List<String> mods = const []]) => calls.add('key $name ${mods.join('+')}'.trim());
  @override
  // ignore: must_call_super
  void dispose() {}
}

AgentState sampleState({bool playing = true, int? volume = 40, bool muted = false, List<PlayerInfo>? players}) => AgentState(
      host: 'living-room',
      backend: 'linux-mpris',
      version: '1',
      current: 'a',
      players: players ??
          [
            PlayerInfo(id: 'a', app: 'Spotify', title: 'Everything In Its Right Place', artist: 'Radiohead', playing: playing, posMs: 83000, durMs: 251000, canSeek: true),
          ],
      volume: volume,
      muted: muted,
    );

final shotKey = GlobalKey();

/// Wraps a screen in the app theme at phone size. Screenshots go to $SHOT_DIR when it is set.
Future<void> pumpScreen(WidgetTester tester, Widget child, {bool dark = true, Size size = const Size(390, 780)}) async {
  tester.view.physicalSize = size * 2;
  tester.view.devicePixelRatio = 2;
  addTearDown(tester.view.reset);
  await tester.pumpWidget(RepaintBoundary(
    key: shotKey,
    child: MaterialApp(debugShowCheckedModeBanner: false, theme: lightTheme(), darkTheme: darkTheme(), themeMode: dark ? ThemeMode.dark : ThemeMode.light, home: Scaffold(body: SafeArea(child: child))),
  ));
  await tester.pump(const Duration(milliseconds: 50));
}

Future<void> shot(WidgetTester tester, String name) async {
  final dir = Platform.environment['SHOT_DIR'];
  if (dir == null) return;
  await tester.runAsync(() async {
    final boundary = shotKey.currentContext!.findRenderObject() as RenderRepaintBoundary;
    final image = await boundary.toImage(pixelRatio: 2);
    final bytes = await image.toByteData(format: ui.ImageByteFormat.png);
    await File('$dir/$name.png').writeAsBytes(bytes!.buffer.asUint8List());
  });
}
