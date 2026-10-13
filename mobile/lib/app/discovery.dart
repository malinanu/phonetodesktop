import 'dart:async';
import 'dart:io';

import 'package:nsd/nsd.dart' as nsd;

/// A PC seen on the local network. [id] is its stable identity (it does not change when its IP does).
class Sighting {
  const Sighting(this.id, this.host, this.port);
  final String id, host;
  final int port;
}

abstract class Discovery {
  /// Look for PCs for a few seconds.
  Future<List<Sighting>> scan({Duration timeout = const Duration(seconds: 4)});
}

/// Uses the phone's own network-service discovery (Android NsdManager, iOS Bonjour), which needs no multicast lock.
class NsdDiscovery implements Discovery {
  @override
  Future<List<Sighting>> scan({Duration timeout = const Duration(seconds: 4)}) async {
    final found = <String, Sighting>{};
    nsd.Discovery? d;
    try {
      d = await nsd.startDiscovery('_phoneremote._tcp', autoResolve: true, ipLookupType: nsd.IpLookupType.v4);
      void collect() {
        for (final s in d!.services) {
          final id = s.txt?['id'];
          final host = s.addresses?.where((a) => a.type == InternetAddressType.IPv4).map((a) => a.address).firstOrNull;
          if (id != null && host != null && s.port != null) {
            found[String.fromCharCodes(id)] = Sighting(String.fromCharCodes(id), host, s.port!);
          }
        }
      }

      d.addListener(collect);
      await Future<void>.delayed(timeout);
      collect();
    } catch (_) {
      // discovery is a convenience: the saved address keeps working
    } finally {
      if (d != null) {
        try {
          await nsd.stopDiscovery(d);
        } catch (_) {}
      }
    }
    return found.values.toList();
  }
}
