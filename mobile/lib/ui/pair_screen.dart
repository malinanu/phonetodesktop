import 'dart:io';

import 'package:flutter/material.dart';
import 'package:mobile_scanner/mobile_scanner.dart';

import '../app/connection.dart';
import '../app/theme.dart';
import '../core/protocol.dart';
import '../core/session.dart';
import 'widgets.dart';

typedef Scanner = Future<PairingInfo?> Function(BuildContext context);
typedef Pairer = Future<PairResult> Function(PairingInfo info, ConnectionController link, void Function() onPending);

Future<PairResult> _realPair(PairingInfo info, ConnectionController link, void Function() onPending) => pairWithPc(
      info: info,
      identity: link.identity,
      deviceName: Platform.isIOS ? 'iPhone' : 'Android phone',
      platform: Platform.isIOS ? 'ios' : 'android',
      onPending: onPending,
    );

/// First run, or "Add a PC": scan the code shown on the PC, then wait for its owner to allow this phone.
class PairScreen extends StatefulWidget {
  const PairScreen({super.key, required this.link, this.onPaired, this.scan = scanQr, this.pair = _realPair, this.onBluetooth});
  final ConnectionController link;
  final VoidCallback? onPaired;
  final Scanner scan;
  final Pairer pair;

  /// Android only: use the phone as a Bluetooth remote instead of pairing over Wi-Fi.
  final VoidCallback? onBluetooth;

  @override
  State<PairScreen> createState() => _PairScreenState();
}

enum _Step { idle, connecting, waiting }

class _PairScreenState extends State<PairScreen> {
  _Step _step = _Step.idle;
  String? _problem;
  String _pcName = '';

  Future<void> _start() async {
    setState(() => _problem = null);
    final info = await widget.scan(context);
    if (info == null || !mounted) return;
    if (info.fingerprint.isEmpty) {
      setState(() => _problem = 'This PC does not offer a secure connection yet. Update Phone Remote on the PC, then scan its new code.');
      return;
    }
    setState(() {
      _step = _Step.connecting;
      _pcName = info.name;
    });
    PairResult? result;
    try {
      result = await widget.pair(info, widget.link, () {
        if (mounted) setState(() => _step = _Step.waiting);
      });
    } on InsecurePc {
      _fail('This PC does not offer a secure connection yet. Update Phone Remote on the PC.');
      return;
    } catch (_) {
      _fail('Could not reach ${info.name}. Are the phone and the PC on the same Wi-Fi?');
      return;
    }
    if (!mounted) return;
    switch (result) {
      case PairResult.approved:
        await widget.link.addPairedPc(PcRecord.fromPairing(info));
        if (mounted) setState(() => _step = _Step.idle);
        widget.onPaired?.call();
      case PairResult.denied:
        _fail('${info.name} did not allow this phone.');
      case PairResult.expired:
        _fail('Nobody answered on ${info.name} in time, or the code ran out. Show the code again on the PC and retry.');
      case PairResult.badCode:
        _fail('That code is out of date. On the PC, open Phones to show a fresh one.');
      case PairResult.badKey:
        _fail('This phone could not be set up. Update the app and try again.');
    }
  }

  void _fail(String message) {
    if (!mounted) return;
    setState(() {
      _step = _Step.idle;
      _problem = message;
    });
  }

  @override
  Widget build(BuildContext context) {
    final c = context.pr;
    final busy = _step != _Step.idle;
    return Scaffold(
      appBar: widget.onPaired != null && Navigator.canPop(context) ? AppBar() : null,
      body: SafeArea(
        child: ListView(padding: const EdgeInsets.fromLTRB(24, 24, 24, 24), children: [
          const Kicker('Setup'),
          const SizedBox(height: 8),
          const Text('Take the remote.', style: TextStyle(fontSize: 40, fontWeight: FontWeight.w700, height: .95)),
          const SizedBox(height: 14),
          Text('Pause, skip and seek your computer’s media from the couch. Pair once; it reconnects by itself.', style: TextStyle(color: c.dim, fontSize: 17, height: 1.35)),
          const SizedBox(height: 22),
          for (final (i, t) in ['Open Phone Remote on your computer', 'Open the dashboard and go to Phones', 'Scan the code, then tap Allow on the computer'].indexed)
            Padding(
              padding: const EdgeInsets.symmetric(vertical: 10),
              child: Row(children: [
                SizedBox(width: 38, child: Text('${i + 1}', style: TextStyle(color: c.accentText, fontSize: 26, fontWeight: FontWeight.w700))),
                Expanded(child: Text(t, style: const TextStyle(fontSize: 16))),
              ]),
            ),
          const SizedBox(height: 18),
          if (busy)
            Card2(
              child: Row(children: [
                const SizedBox(width: 26, height: 26, child: CircularProgressIndicator(strokeWidth: 3)),
                const SizedBox(width: 16),
                Expanded(child: Text(_step == _Step.waiting ? 'Waiting for you to allow this phone on $_pcName…' : 'Connecting to $_pcName…', style: const TextStyle(fontSize: 16, fontWeight: FontWeight.w600))),
              ]),
            )
          else
            Column(crossAxisAlignment: CrossAxisAlignment.stretch, children: [
              FilledButton.icon(onPressed: _start, icon: const Icon(Icons.qr_code_scanner_rounded), label: const Text('Scan QR code')),
              if (widget.onBluetooth != null)
                Padding(
                  padding: const EdgeInsets.only(top: 8),
                  child: TextButton.icon(onPressed: widget.onBluetooth, icon: const Icon(Icons.bluetooth_rounded), label: const Text('Use Bluetooth instead')),
                ),
            ]),
          if (_problem != null)
            Padding(
              padding: const EdgeInsets.only(top: 16),
              child: Container(
                padding: const EdgeInsets.all(14),
                decoration: BoxDecoration(color: c.bad.withValues(alpha: .12), borderRadius: BorderRadius.circular(14), border: Border.all(color: c.bad.withValues(alpha: .5))),
                child: Text(_problem!, style: TextStyle(color: c.ink, fontSize: 15, height: 1.35)),
              ),
            ),
        ]),
      ),
    );
  }
}

/// Opens the camera and returns the first Phone Remote code it sees.
Future<PairingInfo?> scanQr(BuildContext context) => Navigator.of(context).push<PairingInfo>(MaterialPageRoute(builder: (_) => const ScanPage()));

class ScanPage extends StatefulWidget {
  const ScanPage({super.key});
  @override
  State<ScanPage> createState() => _ScanPageState();
}

class _ScanPageState extends State<ScanPage> {
  final _controller = MobileScannerController(formats: const [BarcodeFormat.qrCode], detectionSpeed: DetectionSpeed.noDuplicates);
  bool _notOurs = false;

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      backgroundColor: Colors.black,
      appBar: AppBar(backgroundColor: Colors.black, foregroundColor: Colors.white, title: const Text('Scan the code on your PC')),
      body: Stack(children: [
        MobileScanner(
          controller: _controller,
          errorBuilder: (context, error) => Center(
            child: Padding(
              padding: const EdgeInsets.all(28),
              child: Text(
                error.errorCode == MobileScannerErrorCode.permissionDenied
                    ? 'Allow camera access in your phone’s settings to scan the code.'
                    : 'The camera is not available.',
                textAlign: TextAlign.center,
                style: const TextStyle(color: Colors.white, fontSize: 18),
              ),
            ),
          ),
          onDetect: (capture) {
            for (final b in capture.barcodes) {
              final info = PairingInfo.parse(b.rawValue);
              if (info != null) {
                Navigator.of(context).pop(info);
                return;
              }
            }
            if (!_notOurs) setState(() => _notOurs = true);
          },
        ),
        if (_notOurs)
          const Align(
            alignment: Alignment.bottomCenter,
            child: Padding(padding: EdgeInsets.all(24), child: Text('That is not a Phone Remote code', style: TextStyle(color: Colors.white, fontSize: 16, fontWeight: FontWeight.w600))),
          ),
      ]),
    );
  }
}
