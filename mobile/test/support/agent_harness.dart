// Runs the real PC agent binary (mock media backend) in a throwaway home folder, for end-to-end tests.
// Build it first:  cd agent && cargo build
import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:phoneremote/core/protocol.dart';

final agentBinary = File('../agent/target/debug/phone-remote');
final agentSkip = agentBinary.existsSync() ? false : 'build the agent first: cd agent && cargo build';

/// Runs the agent in a throwaway home folder and exposes its QR and dashboard API.
class AgentUnderTest {
  AgentUnderTest._(this.process, this.port, this.qr, this.home);

  final Process process;
  final int port;
  final PairingInfo qr;
  final Directory home;

  static Future<AgentUnderTest> start() async {
    final probe = await ServerSocket.bind(InternetAddress.loopbackIPv4, 0);
    final port = probe.port;
    await probe.close();
    final home = await Directory.systemTemp.createTemp('pr-live-');
    final process = await Process.start(
      agentBinary.absolute.path,
      ['serve', '--console', '--mock', '--no-mdns', '--port', '$port'],
      environment: {'HOME': home.path, 'XDG_CONFIG_HOME': '${home.path}/.config', 'XDG_DATA_HOME': '${home.path}/.local/share'},
    );
    process.stderr.drain<void>();
    final qr = Completer<PairingInfo>();
    process.stdout.transform(utf8.decoder).transform(const LineSplitter()).listen((line) {
      final info = PairingInfo.parse(line);
      if (info != null && !qr.isCompleted) qr.complete(info);
    });
    final info = await qr.future.timeout(const Duration(seconds: 15));
    return AgentUnderTest._(process, port, info, home);
  }

  Future<void> stop() async {
    process.kill();
    await process.exitCode;
    await home.delete(recursive: true);
  }

  /// The dashboard API, as the owner's browser on this PC would call it.
  Future<dynamic> api(String method, String path) async {
    final client = HttpClient()..findProxy = ((uri) => 'DIRECT');
    final req = await client.openUrl(method, Uri.parse('http://127.0.0.1:$port$path'));
    req.headers.set('x-requested-with', 'phone-remote');
    final res = await req.close();
    final body = await res.transform(utf8.decoder).join();
    client.close();
    expect(res.statusCode, 200, reason: '$method $path -> $body');
    return jsonDecode(body);
  }

  /// Be the owner: wait for a pairing request and press Allow.
  Future<void> approveNextRequest() async {
    for (var i = 0; i < 100; i++) {
      final overview = await api('GET', '/api/overview') as Map<String, dynamic>;
      final pending = overview['pending'] as List;
      if (pending.isNotEmpty) {
        await api('POST', '/api/pending/${Uri.encodeComponent(pending.first['id'] as String)}/approve');
        return;
      }
      await Future<void>.delayed(const Duration(milliseconds: 100));
    }
    fail('no pairing request arrived');
  }
}

