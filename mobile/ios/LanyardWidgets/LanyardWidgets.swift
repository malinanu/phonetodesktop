// The Lanyard Live Activity (lock screen and Dynamic Island) and the Control Center / lock screen buttons.
// The app starts and updates the activity (Runner/MediaBridge.swift); the buttons run LanyardCommandIntent
// (Shared/LanyardShared.swift), which reaches the PC by itself.

import ActivityKit
import AppIntents
import SwiftUI
import WidgetKit

private let accent = Color(red: 1.0, green: 0.604, blue: 0.235) // #ff9a3c
private let cream = Color(red: 0.961, green: 0.922, blue: 0.867) // #f5ebdd
private let dim = Color(red: 0.702, green: 0.647, blue: 0.580) // #b3a594
private let field = Color(red: 0.122, green: 0.102, blue: 0.086) // #1f1a16

@main
struct LanyardWidgetsBundle: WidgetBundle {
    var body: some Widget {
        LanyardLiveActivity()
        if #available(iOSApplicationExtension 18.0, *) {
            LanyardPlayPauseControl()
            LanyardNextControl()
        }
    }
}

// MARK: - Live Activity

struct LanyardLiveActivity: Widget {
    var body: some WidgetConfiguration {
        ActivityConfiguration(for: LanyardActivityAttributes.self) { context in
            LockScreenView(state: context.state, pcName: context.attributes.pcName)
                .padding(16)
                .activityBackgroundTint(field)
                .activitySystemActionForegroundColor(accent)
        } dynamicIsland: { context in
            DynamicIsland {
                DynamicIslandExpandedRegion(.leading) {
                    Image(systemName: context.state.mode == "bluetooth" ? "dot.radiowaves.left.and.right" : "music.note")
                        .font(.title2)
                        .foregroundColor(accent)
                        .padding(.leading, 4)
                }
                DynamicIslandExpandedRegion(.center) {
                    VStack(alignment: .leading, spacing: 2) {
                        Text(context.state.title).font(.headline).lineLimit(1)
                        Text(subtitle(context.state, context.attributes.pcName)).font(.caption).foregroundColor(dim).lineLimit(1)
                    }
                }
                DynamicIslandExpandedRegion(.bottom) {
                    Buttons(state: context.state).padding(.top, 4)
                }
            } compactLeading: {
                Image(systemName: context.state.playing ? "play.fill" : "pause.fill").foregroundColor(accent)
            } compactTrailing: {
                Text(context.state.title).font(.caption2).lineLimit(1).frame(maxWidth: 72)
            } minimal: {
                Image(systemName: "playpause.fill").foregroundColor(accent)
            }
            .keylineTint(accent)
        }
    }
}

private func subtitle(_ s: LanyardActivityAttributes.ContentState, _ pc: String) -> String {
    if s.mode == "bluetooth" { return "Bluetooth · \(pc)" }
    let parts = [s.artist.isEmpty ? s.app : s.artist, pc].filter { !$0.isEmpty }
    return parts.joined(separator: " · ")
}

private struct LockScreenView: View {
    let state: LanyardActivityAttributes.ContentState
    let pcName: String

    var body: some View {
        HStack(spacing: 14) {
            VStack(alignment: .leading, spacing: 3) {
                Text("LANYARD").font(.caption2.weight(.bold)).kerning(1.2).foregroundColor(dim)
                Text(state.title).font(.headline).foregroundColor(cream).lineLimit(1)
                Text(subtitle(state, pcName)).font(.subheadline).foregroundColor(dim).lineLimit(1)
            }
            Spacer(minLength: 8)
            Buttons(state: state)
        }
    }
}

private struct Buttons: View {
    let state: LanyardActivityAttributes.ContentState

    var body: some View {
        HStack(spacing: 10) {
            Button(intent: LanyardCommandIntent(.prev)) {
                Image(systemName: "backward.fill").frame(width: 40, height: 40)
            }
            .tint(cream)
            Button(intent: LanyardCommandIntent(.playPause)) {
                Image(systemName: state.mode == "bluetooth" ? "playpause.fill" : (state.playing ? "pause.fill" : "play.fill"))
                    .frame(width: 46, height: 46)
            }
            .tint(accent)
            Button(intent: LanyardCommandIntent(.next)) {
                Image(systemName: "forward.fill").frame(width: 40, height: 40)
            }
            .tint(cream)
        }
        .buttonStyle(.bordered)
        .buttonBorderShape(.circle)
        .font(.title3)
    }
}

// MARK: - Control Center and lock screen buttons (iOS 18)

@available(iOSApplicationExtension 18.0, *)
struct LanyardPlayPauseControl: ControlWidget {
    var body: some ControlWidgetConfiguration {
        StaticControlConfiguration(kind: "app.phoneremote.control.playpause") {
            ControlWidgetButton(action: LanyardCommandIntent(.playPause)) {
                Label("Play or pause", systemImage: "playpause.fill")
            }
        }
        .displayName("Play or pause on PC")
        .description("Plays or pauses on the computer Lanyard is connected to.")
    }
}

@available(iOSApplicationExtension 18.0, *)
struct LanyardNextControl: ControlWidget {
    var body: some ControlWidgetConfiguration {
        StaticControlConfiguration(kind: "app.phoneremote.control.next") {
            ControlWidgetButton(action: LanyardCommandIntent(.next)) {
                Label("Next", systemImage: "forward.fill")
            }
        }
        .displayName("Next on PC")
        .description("Skips to the next track on the computer Lanyard is connected to.")
    }
}
