import 'dart:async';

import 'package:flutter/material.dart';

import '../app/connection.dart';
import '../app/settings.dart';
import '../app/theme.dart';
import '../core/protocol.dart';
import 'widgets.dart';

/// What is playing on the PC, with transport and volume.
class RemoteScreen extends StatefulWidget {
  const RemoteScreen({super.key, required this.link, required this.settings});
  final ConnectionController link;
  final AppSettings settings;

  @override
  State<RemoteScreen> createState() => _RemoteScreenState();
}

class _RemoteScreenState extends State<RemoteScreen> {
  Timer? _tick;
  double? _dragPos; // ms while the user drags the progress bar
  double? _dragVol;
  DateTime _volSent = DateTime(0);

  @override
  void initState() {
    super.initState();
    // The PC reports about once a second; move the bar smoothly in between.
    _tick = Timer.periodic(const Duration(milliseconds: 250), (_) {
      if (mounted && widget.link.state?.nowPlaying?.playing == true && _dragPos == null) setState(() {});
    });
  }

  @override
  void dispose() {
    _tick?.cancel();
    super.dispose();
  }

  ConnectionController get link => widget.link;
  AppSettings get settings => widget.settings;

  void _do(VoidCallback action) {
    buzz(settings);
    action();
  }

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: Listenable.merge([link, settings]),
      builder: (context, _) {
        final c = context.pr;
        final state = link.state;
        final p = state?.nowPlaying;
        final online = link.status == LinkStatus.connected;
        final dur = (p?.durMs ?? 0).toDouble();
        final pos = _dragPos ?? (p == null ? 0.0 : link.estimatedPosMs(p).toDouble());
        final skip = settings.skip;
        return ListView(padding: const EdgeInsets.fromLTRB(20, 8, 20, 24), children: [
          if (state != null && state.players.length > 1) _playerPicker(context, state),
          Card2(
            child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
              Kicker(p == null ? 'Now playing' : (p.playing ? '${p.app} · playing' : '${p.app} · paused')),
              const SizedBox(height: 10),
              Text(p == null ? 'Nothing playing' : (p.title.isEmpty ? p.app : p.title), maxLines: 2, overflow: TextOverflow.ellipsis, style: const TextStyle(fontSize: 28, fontWeight: FontWeight.w700, height: 1.1)),
              const SizedBox(height: 6),
              Text(p == null ? 'Start something on your PC. The buttons below still work.' : (p.artist.isEmpty ? ' ' : p.artist), maxLines: 2, overflow: TextOverflow.ellipsis, style: TextStyle(color: c.dim, fontSize: 16)),
              const SizedBox(height: 14),
              Semantics(
                label: 'Progress',
                child: Slider(
                  value: dur > 0 ? pos.clamp(0, dur) : 0,
                  max: dur > 0 ? dur : 1,
                  onChanged: online && p != null && p.canSeek && dur > 0 ? (v) => setState(() => _dragPos = v) : null,
                  onChangeEnd: (v) {
                    link.seekAbs(v.round());
                    Future<void>.delayed(const Duration(milliseconds: 700), () => mounted ? setState(() => _dragPos = null) : null);
                  },
                ),
              ),
              Row(mainAxisAlignment: MainAxisAlignment.spaceBetween, children: [
                Text(clock(pos.round()), style: TextStyle(color: c.dim, fontSize: 13)),
                Text(dur > 0 ? clock(dur.round()) : '', style: TextStyle(color: c.dim, fontSize: 13)),
              ]),
            ]),
          ),
          const SizedBox(height: 20),
          Row(mainAxisAlignment: MainAxisAlignment.spaceEvenly, crossAxisAlignment: CrossAxisAlignment.start, children: [
            _fit(PadButton(icon: Icons.skip_previous_rounded, label: 'Previous', onPressed: online ? () => _do(link.prev) : null)),
            _fit(PadButton(icon: Icons.replay_rounded, label: '−${skip}s', onPressed: online ? () => _do(() => link.seekRel(-skip)) : null)),
            _fit(PadButton(icon: p?.playing == true ? Icons.pause_rounded : Icons.play_arrow_rounded, label: p?.playing == true ? 'Pause' : 'Play', filled: true, size: 84, onPressed: online ? () => _do(link.playPause) : null)),
            _fit(PadButton(icon: Icons.forward_rounded, label: '+${skip}s', onPressed: online ? () => _do(() => link.seekRel(skip)) : null)),
            _fit(PadButton(icon: Icons.skip_next_rounded, label: 'Next', onPressed: online ? () => _do(link.next) : null)),
          ]),
          const SizedBox(height: 24),
          Card2(child: _volume(context, state, online)),
        ]);
      },
    );
  }

  /// Shrinks a button to fit when the row is narrow (small phones, large system fonts).
  Widget _fit(Widget w) => Flexible(child: FittedBox(fit: BoxFit.scaleDown, child: w));

  Widget _playerPicker(BuildContext context, AgentState state) {
    final c = context.pr;
    return Padding(
      padding: const EdgeInsets.only(bottom: 14),
      child: Card2(
        padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 4),
        child: DropdownButtonHideUnderline(
          child: DropdownButton<String>(
            isExpanded: true,
            value: state.players.any((p) => p.id == state.current) ? state.current : state.players.first.id,
            dropdownColor: c.surface2,
            style: TextStyle(fontFamily: 'Bricolage', color: c.ink, fontSize: 16, fontWeight: FontWeight.w600),
            items: [for (final p in state.players) DropdownMenuItem(value: p.id, child: Text('${p.app}${p.title.isEmpty ? '' : ' — ${p.title}'}', overflow: TextOverflow.ellipsis))],
            onChanged: (id) => id == null ? null : link.selectPlayer(id),
          ),
        ),
      ),
    );
  }

  Widget _volume(BuildContext context, AgentState? state, bool online) {
    final c = context.pr;
    final level = (_dragVol ?? state?.volume?.toDouble() ?? 0).clamp(0.0, 100.0);
    final muted = state?.muted == true;
    return Row(children: [
      IconButton(
        tooltip: muted ? 'Unmute' : 'Mute',
        onPressed: online ? () => _do(link.mute) : null,
        icon: Icon(muted ? Icons.volume_off_rounded : Icons.volume_up_rounded, color: muted ? c.bad : c.ink),
      ),
      Expanded(
        child: state?.volume == null
            ? Row(mainAxisAlignment: MainAxisAlignment.center, children: [
                IconButton(tooltip: 'Quieter', onPressed: online ? () => _do(() => link.volume(-settings.volStep)) : null, icon: const Icon(Icons.remove_rounded)),
                Text('Volume', style: TextStyle(color: c.dim)),
                IconButton(tooltip: 'Louder', onPressed: online ? () => _do(() => link.volume(settings.volStep)) : null, icon: const Icon(Icons.add_rounded)),
              ])
            : Semantics(
                label: 'Volume',
                child: Slider(
                  value: level,
                  max: 100,
                  divisions: 50,
                  onChanged: online
                      ? (v) {
                          setState(() => _dragVol = v);
                          final now = DateTime.now();
                          if (now.difference(_volSent) > const Duration(milliseconds: 90)) {
                            _volSent = now;
                            link.volumeSet(v.round());
                          }
                        }
                      : null,
                  onChangeEnd: (v) {
                    link.volumeSet(v.round());
                    Future<void>.delayed(const Duration(milliseconds: 500), () => mounted ? setState(() => _dragVol = null) : null);
                  },
                ),
              ),
      ),
      SizedBox(width: 44, child: Text(state?.volume == null ? '' : '${level.round()}', textAlign: TextAlign.end, style: TextStyle(color: c.dim, fontWeight: FontWeight.w600))),
    ]);
  }
}
