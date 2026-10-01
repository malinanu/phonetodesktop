// The connection controller against the real PC agent: connect, follow state, lose the phone, find a moved PC.
import 'dart:async';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:phoneremote/app/connection.dart';
import 'package:phoneremote/app/discovery.dart';
import 'package:phoneremote/app/pc_store.dart';
import 'package:phoneremote/core/agent_link.dart';
import 'package:phoneremote/core/identity.dart';
import 'package:phoneremote/core/session.dart';
import 'package:phoneremote/core/spki.dart';

import 'support/agent_harness.dart';

class FakeDiscovery implements Discovery {
  FakeDiscovery(this.sightings);
  final List<Sighting> sightings;
  int scans = 0;
  @override
  Future<List<Sighting>> scan({Duration timeout = const Duration(seconds: 4)}) async {
    scans++;
    return sightings;
  }
}

/// Waits until [test] is true for the controller (checked on every change).
Future<void> until(ConnectionController c, bool Function() test, {Duration timeout = const Duration(seconds: 10)}) {
  if (test()) return Future.value();
  final done = Completer<void>();
  void listener() {
    if (test() && !done.isCompleted) done.complete();
  }

  c.addListener(listener);
  return done.future.timeout(timeout, onTimeout: () => fail('timed out; status is ${c.status}')).whenComplete(() => c.removeListener(listener));
}

void main() {
  late AgentUnderTest agent;

  setUpAll(() async {
    if (agentSkip != false) return;
    agent = await AgentUnderTest.start();
  });
  tearDownAll(() async {
    if (agentSkip == false) await agent.stop();
  });

  Future<ConnectionController> pairedController({PcRecord? override, Discovery? discovery, SocketOpener opener = openPinnedSocket}) async {
    final c = ConnectionController(
      pcStore: MemoryPcStore(),
      identityStore: MemoryIdentityStore(),
      discovery: discovery,
      opener: opener,
      retryDelays: const [Duration(milliseconds: 20)],
    );
    await c.load();
    expect(c.status, LinkStatus.idle);
    final owner = agent.approveNextRequest();
    final result = await pairWithPc(info: agent.qr, identity: c.identity, deviceName: 'Test phone');
    await owner;
    expect(result, PairResult.approved);
    await c.addPairedPc(override ?? PcRecord.fromPairing(agent.qr));
    return c;
  }

  test('connects after pairing, follows the PC and sends commands', () async {
    final c = await pairedController();
    addTearDown(c.dispose);
    await until(c, () => c.status == LinkStatus.connected && c.state != null);
    expect(c.state!.backend, 'mock');
    expect(c.inputAllowed, isTrue);
    final playing = c.state!.nowPlaying!.playing;
    c.session!.playPause();
    await until(c, () => c.state!.nowPlaying!.playing != playing);
    // the progress estimate moves with time while playing and never passes the end
    final p = c.state!.nowPlaying!;
    expect(c.estimatedPosMs(p), greaterThanOrEqualTo(p.posMs));
    expect(c.estimatedPosMs(p), lessThanOrEqualTo(p.durMs));
  }, skip: agentSkip);

  test('a saved PC and the identity come back after a restart', () async {
    final pcs = MemoryPcStore();
    final ids = MemoryIdentityStore();
    final first = ConnectionController(pcStore: pcs, identityStore: ids, retryDelays: const [Duration(milliseconds: 20)]);
    await first.load();
    final owner = agent.approveNextRequest();
    expect(await pairWithPc(info: agent.qr, identity: first.identity, deviceName: 'Phone 2'), PairResult.approved);
    await owner;
    await first.addPairedPc(PcRecord.fromPairing(agent.qr));
    await until(first, () => first.status == LinkStatus.connected);
    final deviceId = first.identity.deviceId;
    first.dispose();

    final second = ConnectionController(pcStore: pcs, identityStore: ids, retryDelays: const [Duration(milliseconds: 20)]);
    await second.load();
    addTearDown(second.dispose);
    expect(second.identity.deviceId, deviceId);
    expect(second.active!.id, agent.qr.pcId);
    await until(second, () => second.status == LinkStatus.connected);
  }, skip: agentSkip);

  test('when the PC removes the phone it says so and stops retrying', () async {
    final c = await pairedController();
    addTearDown(c.dispose);
    await until(c, () => c.status == LinkStatus.connected);
    await agent.api('DELETE', '/api/devices/${Uri.encodeComponent(c.identity.deviceId)}');
    await until(c, () => c.status == LinkStatus.revoked);
    await Future<void>.delayed(const Duration(milliseconds: 300));
    expect(c.status, LinkStatus.revoked, reason: 'it must not keep retrying a removed phone');
  }, skip: agentSkip);

  test('a PC whose key differs from the pinned one is reported as changed, not retried forever', () async {
    final real = PcRecord.fromPairing(agent.qr);
    final c = await pairedController(override: PcRecord(id: real.id, name: real.name, host: real.host, port: real.port, fingerprint: fingerprintOfSpki([9, 9, 9])));
    addTearDown(c.dispose);
    await until(c, () => c.status == LinkStatus.keyChanged);
  }, skip: agentSkip);

  test('a PC that moved is found again by its identity and the new address is remembered', () async {
    final real = PcRecord.fromPairing(agent.qr);
    // The saved address points nowhere (a documentation address); the real one only comes from discovery.
    final moved = PcRecord(id: real.id, name: real.name, host: '203.0.113.9', port: 9, fingerprint: real.fingerprint);
    final discovery = FakeDiscovery([Sighting(real.id, real.host, real.port), const Sighting('someone-else', '10.0.0.1', 1)]);
    Future<WebSocket> opener(String host, int port, String fp) =>
        host == '203.0.113.9' ? Future.error(const SocketException('unreachable')) : openPinnedSocket(host, port, fp);
    final c = await pairedController(override: moved, discovery: discovery, opener: opener);
    addTearDown(c.dispose);
    await until(c, () => c.status == LinkStatus.connected);
    expect(discovery.scans, greaterThan(0));
    expect(c.active!.host, real.host, reason: 'the new address is kept for next time');
    expect(c.pcs.single.host, real.host);
  }, skip: agentSkip);

  test('forgetting the only PC returns to the start', () async {
    final c = await pairedController();
    addTearDown(c.dispose);
    await until(c, () => c.status == LinkStatus.connected);
    await c.forget(c.active!.id);
    expect(c.status, LinkStatus.idle);
    expect(c.pcs, isEmpty);
    expect(c.session, isNull);
  }, skip: agentSkip);
}
