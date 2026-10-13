import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'spki.dart';

/// Opens a WebSocket to the PC's agent over TLS and trusts exactly one thing: the public key whose
/// SHA-256 fingerprint came from the pairing QR. Host name, dates and certificate chain are ignored on purpose
/// (the certificate is self-signed); the TLS handshake still proves the server holds the matching private key.
typedef SocketOpener = Future<WebSocket> Function(String host, int port, String fingerprint);

Future<WebSocket> openPinnedSocket(String host, int port, String fingerprint) {
  const timeout = Duration(seconds: 8);
  // No built-in roots: nothing can be trusted by accident, so the callback below always decides.
  final client = HttpClient(context: SecurityContext(withTrustedRoots: false))
    ..connectionTimeout = timeout
    // The PC is on the local network: never send this through an HTTP proxy configured in the environment.
    ..findProxy = ((uri) => 'DIRECT')
    ..badCertificateCallback = (cert, host, port) => certMatchesPin(cert.der, fingerprint);
  final hostPart = host.contains(':') ? '[$host]' : host;
  return WebSocket.connect('wss://$hostPart:$port/ws', customClient: client).timeout(timeout);
}

class LinkClosed implements Exception {
  @override
  String toString() => 'The connection to the PC closed';
}

class _Waiter {
  _Waiter(this.test);
  final bool Function(Map<String, dynamic>) test;
  final Completer<Map<String, dynamic>> completer = Completer();
}

/// One WebSocket to the agent, speaking JSON text frames. During the login handshake messages are
/// buffered so none can be missed; afterwards they flow through [messages].
class AgentLink {
  AgentLink(this._socket) {
    _socket.listen(_onData, onDone: _onDone, onError: (_) => _onDone(), cancelOnError: true);
  }

  final WebSocket _socket;
  final List<Map<String, dynamic>> _buffer = [];
  final List<_Waiter> _waiters = [];
  final StreamController<Map<String, dynamic>> _stream = StreamController.broadcast();
  final Completer<void> _done = Completer();
  bool _streaming = false;
  bool _closed = false;

  Stream<Map<String, dynamic>> get messages => _stream.stream;
  Future<void> get done => _done.future;
  bool get isClosed => _closed;

  void _onData(dynamic data) {
    // After close() the PC may still deliver a frame that was already on its way: ignore it.
    if (_closed || data is! String) return;
    final Map<String, dynamic> m;
    try {
      final decoded = jsonDecode(data);
      if (decoded is! Map<String, dynamic>) return;
      m = decoded;
    } on FormatException {
      return;
    }
    for (final w in _waiters) {
      if (w.test(m)) {
        _waiters.remove(w);
        w.completer.complete(m);
        return;
      }
    }
    (_streaming ? _stream.add : _buffer.add)(m);
  }

  void _onDone() {
    if (_closed) return;
    _closed = true;
    for (final w in _waiters) {
      w.completer.completeError(LinkClosed());
    }
    _waiters.clear();
    _stream.close();
    if (!_done.isCompleted) _done.complete();
  }

  void send(Map<String, dynamic> message) {
    if (!_closed) _socket.add(jsonEncode(message));
  }

  /// The next message accepted by [test] (checking ones that already arrived first).
  Future<Map<String, dynamic>> waitFor(bool Function(Map<String, dynamic>) test, {Duration timeout = const Duration(seconds: 10)}) {
    final i = _buffer.indexWhere(test);
    if (i >= 0) return Future.value(_buffer.removeAt(i));
    if (_closed) return Future.error(LinkClosed());
    final w = _Waiter(test);
    _waiters.add(w);
    return w.completer.future.timeout(timeout, onTimeout: () {
      _waiters.remove(w);
      throw TimeoutException('No answer from the PC', timeout);
    });
  }

  /// Switch from handshake buffering to streaming; returns what arrived in the meantime.
  List<Map<String, dynamic>> startStreaming() {
    _streaming = true;
    final pending = List<Map<String, dynamic>>.of(_buffer);
    _buffer.clear();
    return pending;
  }

  Future<void> close() async {
    if (!_closed) await _socket.close();
    _onDone();
  }
}
