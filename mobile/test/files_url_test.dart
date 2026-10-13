import 'package:flutter_test/flutter_test.dart';
import 'package:phoneremote/app/files_url.dart';

void main() {
  test('the built-in page of a computer is its port plus one', () {
    expect(FilesUrl.local('192.168.1.8', 8765), 'http://192.168.1.8:8766/');
    expect(FilesUrl.local('malin-pc.local', 8765), 'http://malin-pc.local:8766/');
    for (final (h, p) in [('', 8765), ('bad host', 8765), ('192.168.1.8', 65535), ('192.168.1.8', 0)]) {
      expect(FilesUrl.local(h, p), isNull, reason: '$h:$p');
    }
  });

  test('only links to that computer’s Send files page are accepted', () {
    for (final ok in ['http://192.168.1.8:8766/', 'http://192.168.1.8:8766', 'http://192.168.1.8:8766/abc-defg-hij', 'http://192.168.1.8:8766/abc-defg-hij?sink=blob']) {
      expect(FilesUrl.isLocalLink(ok, '192.168.1.8', 8765), isTrue, reason: ok);
    }
    for (final bad in [
      null, '', 'https://192.168.1.8:8766/', 'http://192.168.1.9:8766/x', 'http://192.168.1.8:8765/x', 'http://192.168.1.8:87660/x',
      'http://192.168.1.8:8766.evil.com/x', 'http://192.168.1.8:8766@evil.com/', 'http://192.168.1.8:8766/a b', 'javascript:alert(1)', 'http://192.168.1.8:8766/<x>',
    ]) {
      expect(FilesUrl.isLocalLink(bad, '192.168.1.8', 8765), isFalse, reason: '$bad');
    }
  });
}
