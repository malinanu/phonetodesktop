import ActivityKit
import AVFoundation
import Flutter
import MediaPlayer
import UIKit

/// The iPhone side of lib/app/media_controls.dart (channel `app.phoneremote/media`):
/// - a Live Activity on the lock screen and in the Dynamic Island, with play/pause, previous and next (iOS 16.2+;
///   the buttons need iOS 17),
/// - the login details those buttons and the Control Center button use when the app is suspended,
/// - when the user turns it on, the iPhone's own lock-screen player (Now Playing), which needs the app to keep an
///   audio session running: it plays silence while it is shown.
final class MediaBridge: NSObject {
    static var shared: MediaBridge?

    private let channel: FlutterMethodChannel
    private let nowPlaying = NowPlayingController()

    init(messenger: FlutterBinaryMessenger) {
        channel = FlutterMethodChannel(name: "app.phoneremote/media", binaryMessenger: messenger)
        super.init()
        nowPlaying.onCommand = { [weak self] c, v in self?.send(c, v) }
        channel.setMethodCallHandler { [weak self] call, result in
            self?.handle(call, result)
        }
    }

    private func send(_ command: String, _ value: Int?) {
        var args: [String: Any] = ["c": command]
        if let value { args["v"] = value }
        channel.invokeMethod("command", arguments: args)
    }

    private func handle(_ call: FlutterMethodCall, _ result: @escaping FlutterResult) {
        switch call.method {
        case "show":
            let a = call.arguments as? [String: Any] ?? [:]
            if #available(iOS 16.2, *) { LiveActivityController.show(a) }
            if a["iosNowPlaying"] as? Bool == true { nowPlaying.show(a) } else { nowPlaying.stop() }
            result(nil)
        case "hide":
            if #available(iOS 16.2, *) { LiveActivityController.end() }
            nowPlaying.stop()
            result(nil)
        case "requestPermission":
            // Live Activities are allowed per app in the iPhone's Settings; there is nothing to ask here.
            result(true)
        case "setCredentials":
            if let a = call.arguments as? [String: Any],
               let pcId = a["pcId"] as? String, let host = a["host"] as? String, let port = a["port"] as? Int,
               let fp = a["fp"] as? String, let deviceId = a["deviceId"] as? String, let seed = a["seed"] as? String {
                LanyardCredentialStore.save(LanyardCredentials(
                    pcId: pcId, pcName: a["pcName"] as? String ?? "", host: host, port: port, fp: fp,
                    deviceId: deviceId, seed: seed, cert: a["cert"] as? String))
            } else {
                LanyardCredentialStore.save(nil)
            }
            result(nil)
        default:
            result(FlutterMethodNotImplemented)
        }
    }
}

// MARK: - Live Activity

@available(iOS 16.2, *)
enum LiveActivityController {
    static func show(_ a: [String: Any]) {
        guard ActivityAuthorizationInfo().areActivitiesEnabled else { return }
        let pc = a["pc"] as? String ?? "Lanyard"
        let state = LanyardActivityAttributes.ContentState(
            title: a["title"] as? String ?? "",
            artist: a["artist"] as? String ?? "",
            app: a["app"] as? String ?? "",
            playing: a["playing"] as? Bool ?? false,
            connected: a["connected"] as? Bool ?? true,
            mode: a["mode"] as? String ?? "wifi"
        )
        let content = ActivityContent(state: state, staleDate: nil)
        Task {
            let running = Activity<LanyardActivityAttributes>.activities
            if let current = running.first, current.attributes.pcName == pc {
                await current.update(content)
                return
            }
            for old in running { await old.end(nil, dismissalPolicy: .immediate) }
            _ = try? Activity.request(attributes: LanyardActivityAttributes(pcName: pc), content: content, pushType: nil)
        }
    }

    static func end() {
        Task {
            for a in Activity<LanyardActivityAttributes>.activities { await a.end(nil, dismissalPolicy: .immediate) }
        }
    }
}

// MARK: - Now Playing (opt-in)

/// The iPhone's own lock-screen player. iOS only shows it for an app with an active audio session, so this plays
/// silence while it is on. It takes over the phone's audio (music playing on the phone pauses), which is why it is
/// a separate switch in Settings, off by default.
final class NowPlayingController {
    var onCommand: ((String, Int?) -> Void)?

    private var engine: AVAudioEngine?
    private var player: AVAudioPlayerNode?
    private var targets: [(MPRemoteCommand, Any)] = []
    private var interruption: NSObjectProtocol?

    func show(_ a: [String: Any]) {
        if engine == nil { start() }
        let title = a["title"] as? String ?? ""
        let pc = a["pc"] as? String ?? ""
        let app = a["app"] as? String ?? ""
        let playing = a["playing"] as? Bool ?? false
        let canSeek = a["canSeek"] as? Bool ?? false
        var info: [String: Any] = [
            MPMediaItemPropertyTitle: title.isEmpty ? "Lanyard" : title,
            MPMediaItemPropertyArtist: a["artist"] as? String ?? "",
            MPMediaItemPropertyAlbumTitle: [app, pc].filter { !$0.isEmpty }.joined(separator: " · "),
            MPNowPlayingInfoPropertyPlaybackRate: playing ? 1.0 : 0.0,
        ]
        if canSeek {
            info[MPMediaItemPropertyPlaybackDuration] = Double(a["durMs"] as? Int ?? 0) / 1000
            info[MPNowPlayingInfoPropertyElapsedPlaybackTime] = Double(a["posMs"] as? Int ?? 0) / 1000
        }
        MPRemoteCommandCenter.shared().changePlaybackPositionCommand.isEnabled = canSeek
        MPNowPlayingInfoCenter.default().nowPlayingInfo = info
    }

    func stop() {
        guard engine != nil else { return }
        let center = MPRemoteCommandCenter.shared()
        for (command, target) in targets { command.removeTarget(target) }
        targets.removeAll()
        center.changePlaybackPositionCommand.isEnabled = false
        MPNowPlayingInfoCenter.default().nowPlayingInfo = nil
        player?.stop()
        engine?.stop()
        player = nil
        engine = nil
        if let interruption { NotificationCenter.default.removeObserver(interruption) }
        interruption = nil
        try? AVAudioSession.sharedInstance().setActive(false, options: .notifyOthersOnDeactivation)
    }

    private func start() {
        let session = AVAudioSession.sharedInstance()
        do {
            try session.setCategory(.playback, mode: .default, options: [])
            try session.setActive(true)
        } catch {
            return
        }
        let engine = AVAudioEngine()
        let player = AVAudioPlayerNode()
        let format = AVAudioFormat(standardFormatWithSampleRate: 44_100, channels: 2)!
        engine.attach(player)
        engine.connect(player, to: engine.mainMixerNode, format: format)
        guard let silence = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: 44_100) else { return }
        silence.frameLength = 44_100 // zero-filled: one second of silence, looped
        player.scheduleBuffer(silence, at: nil, options: .loops)
        do { try engine.start() } catch { return }
        player.play()
        self.engine = engine
        self.player = player

        let center = MPRemoteCommandCenter.shared()
        func on(_ command: MPRemoteCommand, _ name: String) {
            command.isEnabled = true
            let t = command.addTarget { [weak self] _ in
                self?.onCommand?(name, nil)
                return .success
            }
            targets.append((command, t))
        }
        on(center.togglePlayPauseCommand, "play_pause")
        on(center.playCommand, "play_pause")
        on(center.pauseCommand, "play_pause")
        on(center.nextTrackCommand, "next")
        on(center.previousTrackCommand, "prev")
        let seek = center.changePlaybackPositionCommand.addTarget { [weak self] event in
            guard let e = event as? MPChangePlaybackPositionCommandEvent else { return .commandFailed }
            self?.onCommand?("seek_to", Int(e.positionTime * 1000))
            return .success
        }
        targets.append((center.changePlaybackPositionCommand, seek))

        // A phone call or another app's audio stops the engine; start again when it is over.
        interruption = NotificationCenter.default.addObserver(forName: AVAudioSession.interruptionNotification, object: nil, queue: .main) { [weak self] n in
            guard let raw = n.userInfo?[AVAudioSessionInterruptionTypeKey] as? UInt,
                  AVAudioSession.InterruptionType(rawValue: raw) == .ended, let self, let engine = self.engine else { return }
            try? AVAudioSession.sharedInstance().setActive(true)
            if !engine.isRunning { try? engine.start() }
            self.player?.play()
        }
    }
}
