// Runs the real account service (Python, development mode, throwaway database) for end-to-end tests.
//   cd account && pip install -r requirements-dev.txt
import 'dart:async';
import 'dart:convert';
import 'dart:io';

final _accountDir = Directory('../account');

/// Null when the service can be started here, else the reason to skip.
final String? accountSkip = (() {
  if (!_accountDir.existsSync()) return 'the account/ folder is missing';
  final r = Process.runSync('python3', ['-c', 'import fastapi, uvicorn, sqlalchemy, jwt, cryptography']);
  return r.exitCode == 0 ? null : 'install the account service requirements: pip install -r account/requirements.txt';
})();

class AccountServiceUnderTest {
  AccountServiceUnderTest._(this.process, this.port, this.dir, this._codes);

  final Process process;
  final int port;
  final Directory dir;
  final Map<String, Completer<String>> _codes;

  String get url => 'http://127.0.0.1:$port';

  /// Starts the service. Pass [again] (a stopped instance, stopped with `cleanup: false`) to bring the same service
  /// back: same address, same database, same keys.
  static Future<AccountServiceUnderTest> start({AccountServiceUnderTest? again}) async {
    var port = again?.port ?? 0;
    if (again == null) {
      final probe = await ServerSocket.bind(InternetAddress.loopbackIPv4, 0);
      port = probe.port;
      await probe.close();
    }
    final dir = again?.dir ?? await Directory.systemTemp.createTemp('pr-account-');
    final process = await Process.start(
      'python3',
      ['-m', 'uvicorn', 'app.main:create_app', '--factory', '--host', '127.0.0.1', '--port', '$port', '--no-access-log'],
      workingDirectory: _accountDir.absolute.path,
      environment: {
        'ACCOUNT_DEV_MODE': '1',
        'DATABASE_URL': 'sqlite:///${dir.path}/account.db',
        // fixed, so the service can be restarted without its certificates and sign-ins becoming invalid
        'ACCOUNT_SIGNING_KEYS': 't1:${base64Url.encode(List.filled(32, 7)).replaceAll('=', '')}',
        'ACCOUNT_JWT_SECRET': 'x' * 40,
      },
    );
    final codes = <String, Completer<String>>{};
    final ready = Completer<void>();
    final pattern = RegExp(r'sign-in code for (\S+): (\S+)');
    void scan(String line) {
      if (line.contains('Uvicorn running') && !ready.isCompleted) ready.complete();
      final m = pattern.firstMatch(line);
      if (m != null) {
        final c = codes[m.group(1)!];
        if (c == null || c.isCompleted) codes[m.group(1)!] = Completer<String>();
        codes[m.group(1)!]!.complete(m.group(2)!);
      }
    }

    process.stderr.transform(utf8.decoder).transform(const LineSplitter()).listen(scan);
    process.stdout.drain<void>();
    await ready.future.timeout(const Duration(seconds: 30));
    return AccountServiceUnderTest._(process, port, dir, codes);
  }

  /// The next sign-in code the service "emailed" (development mode writes it to its log). Each code is returned once.
  Future<String> codeFor(String email) async {
    final code = await (_codes[email] ??= Completer<String>()).future.timeout(const Duration(seconds: 10));
    _codes.remove(email);
    return code;
  }

  Future<void> stop({bool cleanup = true}) async {
    process.kill();
    await process.exitCode;
    if (cleanup) await dir.delete(recursive: true);
  }
}
