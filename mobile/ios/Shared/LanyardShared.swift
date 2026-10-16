// Shared by the app (Runner) and the LanyardWidgets extension: the Live Activity's data, the buttons' intent, and a
// small client that logs in to the PC and sends one command. The buttons on the lock screen, in the Dynamic Island
// and in Control Center use it, because iOS suspends the app (and its connection) soon after it leaves the screen.
//
// The login is the same as the Flutter app's (lib/core/session.dart): TLS pinned to the PC's key fingerprint, then
// an Ed25519 signature over "PRv2-auth" 0 pcId 0 deviceId 0 nonce (agent/src/auth.rs).

import ActivityKit
import AppIntents
import CryptoKit
import Foundation
import Security

// MARK: - Live Activity data

@available(iOS 16.1, *)
struct LanyardActivityAttributes: ActivityAttributes {
    struct ContentState: Codable, Hashable {
        var title: String
        var artist: String
        var app: String
        var playing: Bool
        var connected: Bool
        /// "wifi" or "bluetooth"
        var mode: String
    }

    var pcName: String
}

// MARK: - The buttons

@available(iOS 16.0, *)
enum LanyardCommand: String, AppEnum {
    case playPause = "play_pause"
    case next
    case prev

    static var typeDisplayRepresentation: TypeDisplayRepresentation = "Action"
    static var caseDisplayRepresentations: [LanyardCommand: DisplayRepresentation] = [
        .playPause: "Play or pause",
        .next: "Next",
        .prev: "Previous",
    ]
}

/// Play/pause, next or previous on the PC. In a Live Activity it runs in the app's process (LiveActivityIntent),
/// from Control Center in the extension's; both log in to the PC on their own with the stored credentials.
@available(iOS 17.0, *)
struct LanyardCommandIntent: LiveActivityIntent {
    static var title: LocalizedStringResource = "Control the computer"
    static var description = IntentDescription("Play, pause or skip on the computer Lanyard is connected to.")

    @Parameter(title: "Action")
    var command: LanyardCommand

    init() {
        command = .playPause
    }

    init(_ command: LanyardCommand) {
        self.command = command
    }

    func perform() async throws -> some IntentResult {
        // Bluetooth mode has no network login: the app handles it while it runs.
        if let creds = LanyardCredentialStore.load() {
            let state = try? await LanyardClient(credentials: creds).send(command.rawValue)
            if let state {
                await LanyardActivityUpdater.apply(state, pcName: creds.pcName)
            }
        }
        return .result()
    }
}

/// Refreshes the running Live Activity with what the PC reported (only possible in the app's own process).
enum LanyardActivityUpdater {
    static func apply(_ now: LanyardNowPlaying, pcName: String) async {
        guard #available(iOS 16.2, *) else { return }
        let state = LanyardActivityAttributes.ContentState(
            title: now.title.isEmpty ? "Nothing playing" : now.title,
            artist: now.artist,
            app: now.app,
            playing: now.playing,
            connected: true,
            mode: "wifi"
        )
        for activity in Activity<LanyardActivityAttributes>.activities {
            await activity.update(ActivityContent(state: state, staleDate: nil))
        }
        _ = pcName
    }
}

// MARK: - Login details, kept in the keychain

struct LanyardCredentials: Codable {
    var pcId: String
    var pcName: String
    var host: String
    var port: Int
    var fp: String
    var deviceId: String
    var seed: String
    var cert: String?
}

enum LanyardCredentialStore {
    private static let service = "app.phoneremote.lockscreen"
    private static let account = "active-pc"

    /// Shared between the app and the extension when the build is signed with the keychain group
    /// (Info.plist `LanyardKeychainGroup` = `$(AppIdentifierPrefix)app.phoneremote.shared`).
    private static var accessGroup: String? {
        guard let g = Bundle.main.object(forInfoDictionaryKey: "LanyardKeychainGroup") as? String,
              !g.isEmpty, !g.hasPrefix("$("), !g.hasPrefix(".") else { return nil }
        return g
    }

    private static func query(group: String?) -> [String: Any] {
        var q: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
        ]
        if let group { q[kSecAttrAccessGroup as String] = group }
        return q
    }

    /// The shared group first; without it (an unsigned or differently signed build) the app's own keychain.
    private static var groups: [String?] { accessGroup.map { [$0, nil] } ?? [nil] }

    static func save(_ creds: LanyardCredentials?) {
        for g in groups { SecItemDelete(query(group: g) as CFDictionary) }
        guard let creds, let data = try? JSONEncoder().encode(creds) else { return }
        for g in groups {
            var add = query(group: g)
            add[kSecValueData as String] = data
            // Readable while the phone is locked (the lock screen buttons), never in backups or on other devices.
            add[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
            if SecItemAdd(add as CFDictionary, nil) == errSecSuccess { return }
        }
    }

    static func load() -> LanyardCredentials? {
        for g in groups {
            var q = query(group: g)
            q[kSecReturnData as String] = true
            q[kSecMatchLimit as String] = kSecMatchLimitOne
            var out: CFTypeRef?
            if SecItemCopyMatching(q as CFDictionary, &out) == errSecSuccess, let data = out as? Data {
                return try? JSONDecoder().decode(LanyardCredentials.self, from: data)
            }
        }
        return nil
    }
}

// MARK: - One-shot client

struct LanyardNowPlaying: Sendable {
    var title: String
    var artist: String
    var app: String
    var playing: Bool

    static let nothing = LanyardNowPlaying(title: "", artist: "", app: "", playing: false)

    init(title: String, artist: String, app: String, playing: Bool) {
        self.title = title
        self.artist = artist
        self.app = app
        self.playing = playing
    }

    /// The player the PC's `state` message points at (`current`, else the first one), like AgentState.nowPlaying.
    init(state: [String: Any]) {
        let players = state["players"] as? [[String: Any]] ?? []
        let current = state["current"] as? String
        let p = players.first(where: { ($0["id"] as? String) == current }) ?? players.first ?? [:]
        title = p["title"] as? String ?? ""
        artist = p["artist"] as? String ?? ""
        app = p["app"] as? String ?? ""
        playing = p["playing"] as? Bool ?? false
    }
}

/// [operation]'s result, or nil when it takes longer than [seconds] (the operation is then cancelled).
func lanyardWithin<T: Sendable>(_ seconds: Double, _ operation: @escaping @Sendable () async throws -> T) async -> T? {
    await withTaskGroup(of: T?.self) { group in
        group.addTask { try? await operation() }
        group.addTask {
            try? await Task.sleep(nanoseconds: UInt64(seconds * 1_000_000_000))
            return nil
        }
        let first = await group.next() ?? nil
        group.cancelAll()
        return first
    }
}

enum LanyardClientError: Error {
    case badCredentials, refused(String), closed, timeout
}

final class LanyardClient: NSObject, URLSessionDelegate, @unchecked Sendable {
    private let creds: LanyardCredentials
    private var session: URLSession?

    init(credentials: LanyardCredentials) {
        creds = credentials
    }

    /// Logs in, sends [command], and returns what the PC plays afterwards. Gives up after a few seconds.
    func send(_ command: String) async throws -> LanyardNowPlaying {
        defer { session?.invalidateAndCancel() }
        guard let result = await lanyardWithin(6, { try await self.run(command) }) else { throw LanyardClientError.timeout }
        return result
    }

    private func run(_ command: String) async throws -> LanyardNowPlaying {
        guard let seed = Data(base64URL: creds.seed), seed.count == 32,
              let url = URL(string: "wss://\(creds.host.contains(":") ? "[\(creds.host)]" : creds.host):\(creds.port)/ws")
        else { throw LanyardClientError.badCredentials }
        let key = try Curve25519.Signing.PrivateKey(rawRepresentation: seed)

        let config = URLSessionConfiguration.ephemeral
        config.connectionProxyDictionary = [:] // the PC is on the local network: never through a proxy
        config.timeoutIntervalForRequest = 6
        let session = URLSession(configuration: config, delegate: self, delegateQueue: nil)
        self.session = session
        let socket = session.webSocketTask(with: url)
        socket.resume()
        defer { socket.cancel(with: .goingAway, reason: nil) }

        try await sendJSON(socket, ["t": "challenge", "device": creds.deviceId])
        let challenge = try await receive(socket) { $0["t"] as? String == "challenge" }
        guard let nonceText = challenge["nonce"] as? String, let nonce = Data(base64URL: nonceText) else {
            throw LanyardClientError.refused("no challenge")
        }
        var message = Data("PRv2-auth".utf8)
        message.append(0)
        message.append(Data(creds.pcId.utf8))
        message.append(0)
        message.append(Data(creds.deviceId.utf8))
        message.append(0)
        message.append(nonce)
        let signature = try key.signature(for: message)
        var auth: [String: Any] = ["t": "auth_sig", "device": creds.deviceId, "sig": signature.base64URL]
        if let cert = creds.cert { auth["cert"] = cert }
        try await sendJSON(socket, auth)

        var latest: LanyardNowPlaying?
        let verdict = try await receive(socket) { m in
            if m["t"] as? String == "state" { latest = LanyardNowPlaying(state: m) }
            return m["t"] as? String == "auth"
        }
        guard verdict["ok"] as? Bool == true else { throw LanyardClientError.refused(verdict["err"] as? String ?? "refused") }

        try await sendJSON(socket, ["t": "cmd", "c": command])
        // The PC reports the new state shortly after the command. If nothing changes there is no report: then the
        // state from the login is the best we know.
        if let after = await lanyardWithin(1.5, { LanyardNowPlaying(state: try await self.receive(socket) { $0["t"] as? String == "state" }) }) {
            latest = after
        }
        return latest ?? .nothing
    }

    private func sendJSON(_ socket: URLSessionWebSocketTask, _ object: [String: Any]) async throws {
        let data = try JSONSerialization.data(withJSONObject: object)
        try await socket.send(.string(String(decoding: data, as: UTF8.self)))
    }

    private func receive(_ socket: URLSessionWebSocketTask, _ test: ([String: Any]) -> Bool) async throws -> [String: Any] {
        while true {
            try Task.checkCancellation()
            let frame = try await socket.receive()
            let text: String
            switch frame {
            case .string(let s): text = s
            case .data(let d): text = String(decoding: d, as: UTF8.self)
            @unknown default: continue
            }
            guard let obj = try? JSONSerialization.jsonObject(with: Data(text.utf8)) as? [String: Any] else { continue }
            if test(obj) { return obj }
        }
    }

    // Trust exactly the PC's key from the pairing QR: the certificate is self-signed, so names, dates and the chain
    // are ignored; the TLS handshake still proves the PC holds the matching private key.
    func urlSession(_ session: URLSession, didReceive challenge: URLAuthenticationChallenge,
                    completionHandler: @escaping (URLSession.AuthChallengeDisposition, URLCredential?) -> Void) {
        guard challenge.protectionSpace.authenticationMethod == NSURLAuthenticationMethodServerTrust,
              let trust = challenge.protectionSpace.serverTrust,
              let chain = SecTrustCopyCertificateChain(trust) as? [SecCertificate],
              let leaf = chain.first,
              let spki = LanyardSPKI.fromCertificate(SecCertificateCopyData(leaf) as Data),
              Data(SHA256.hash(data: spki)).base64URL == creds.fp
        else {
            completionHandler(.cancelAuthenticationChallenge, nil)
            return
        }
        completionHandler(.useCredential, URLCredential(trust: trust))
    }
}

/// The SubjectPublicKeyInfo of an X.509 certificate (DER), found the same way as lib/core/spki.dart.
enum LanyardSPKI {
    private struct Element { let tag: UInt8; let header: Int; let length: Int }

    private static func element(_ b: [UInt8], _ offset: Int) -> Element? {
        guard offset + 2 <= b.count else { return nil }
        let tag = b[offset], l0 = Int(b[offset + 1])
        if l0 < 0x80 { return Element(tag: tag, header: 2, length: l0) }
        let n = l0 & 0x7f
        guard n > 0, n <= 4, offset + 2 + n <= b.count else { return nil }
        var len = 0
        for i in 0..<n { len = (len << 8) | Int(b[offset + 2 + i]) }
        return Element(tag: tag, header: 2 + n, length: len)
    }

    static func fromCertificate(_ der: Data) -> Data? {
        let b = [UInt8](der)
        guard let c = element(b, 0), c.tag == 0x30, c.header + c.length <= b.count,
              let tbs = element(b, c.header), tbs.tag == 0x30, c.header + tbs.header + tbs.length <= b.count else { return nil }
        var offset = c.header + tbs.header
        let end = offset + tbs.length
        guard var e = element(b, offset) else { return nil }
        if e.tag == 0xA0 { offset += e.header + e.length } // optional version [0]
        for _ in 0..<5 { // serial, signature, issuer, validity, subject
            guard let next = element(b, offset), offset + next.header + next.length <= end else { return nil }
            e = next
            offset += e.header + e.length
        }
        guard let spki = element(b, offset), spki.tag == 0x30, offset + spki.header + spki.length <= end else { return nil }
        return Data(b[offset..<(offset + spki.header + spki.length)])
    }
}

extension Data {
    init?(base64URL s: String) {
        var t = s.replacingOccurrences(of: "-", with: "+").replacingOccurrences(of: "_", with: "/")
        while t.count % 4 != 0 { t += "=" }
        self.init(base64Encoded: t)
    }

    var base64URL: String {
        base64EncodedString().replacingOccurrences(of: "+", with: "-").replacingOccurrences(of: "/", with: "_")
            .replacingOccurrences(of: "=", with: "")
    }
}
