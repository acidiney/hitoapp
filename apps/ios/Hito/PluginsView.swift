import HitoKit
import SwiftUI

struct PluginsView: View {
    @Environment(AppModel.self) private var model
    @State private var state: PluginState?
    @State private var failed = false

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 14) {
                Text("Plugins").font(Theme.title).accessibilityAddTraits(.isHeader)
                if let state {
                    powers(state)
                    connections(state)
                } else if failed {
                    HStack(spacing: 8) {
                        Text("Plugins didn't load.").foregroundStyle(Theme.soft)
                        TextAction(title: "Try again") { Task { await load() } }
                    }
                    .font(Theme.small)
                } else {
                    ProgressView().tint(Theme.soft).frame(maxWidth: .infinity, minHeight: 60)
                }
            }
            .padding(.horizontal, 16)
            .padding(.bottom, 24)
        }
        .scrollIndicators(.hidden)
        .refreshable { await load() }
        .task { if state == nil { await load() } }
    }

    private func powers(_ state: PluginState) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Text("What your plugins power")
                .font(Theme.smallStrong)
                .foregroundStyle(Theme.quiet)
                .padding(.bottom, 4)
                .accessibilityAddTraits(.isHeader)
            ForEach(state.powers) { power in
                let on = !power.providers.isEmpty
                HStack(spacing: 12) {
                    Text(power.name).font(Theme.bodyStrong).frame(maxWidth: .infinity, alignment: .leading)
                    HStack(spacing: 8) {
                        Circle()
                            .fill(on ? Theme.soft : .clear)
                            .strokeBorder(on ? Theme.soft : Theme.quiet, lineWidth: 1.5)
                            .frame(width: 9, height: 9)
                        Text(on ? power.providers.joined(separator: ", ") : "Not offered here yet")
                            .font(Theme.small)
                            .foregroundStyle(on ? Theme.text : Theme.quiet)
                            .lineLimit(1)
                    }
                }
                .frame(minHeight: 40)
                .overlay(alignment: .bottom) { Theme.barLine.frame(height: 1) }
                .accessibilityElement(children: .combine)
            }
        }
    }

    private func connections(_ state: PluginState) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("Your connections").font(Theme.smallStrong).foregroundStyle(Theme.quiet).accessibilityAddTraits(.isHeader)
            if state.connections.isEmpty {
                Text("You haven't connected a plugin yet.").font(Theme.small).foregroundStyle(Theme.soft)
            }
            ForEach(state.connections) { connection in
                NavigationLink(value: connection) {
                    HStack(spacing: 12) {
                        PluginTile(name: connection.name)
                        VStack(alignment: .leading, spacing: 0) {
                            Text(connection.name).font(Theme.bodyStrong).lineLimit(1)
                            Text(status(connection))
                                .font(Theme.small)
                                .foregroundStyle(connection.health == .ok ? Theme.quiet : Theme.warning)
                                .lineLimit(1)
                        }
                        Spacer(minLength: 0)
                        Icon("chevron-right", size: 16).foregroundStyle(Theme.quiet)
                    }
                    .padding(.horizontal, 12)
                    .frame(minHeight: 60)
                    .background(RoundedRectangle(cornerRadius: 16).fill(Theme.field))
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
            }
            VStack(alignment: .leading, spacing: 0) {
                Text("Available on this server").font(Theme.bodyStrong)
                Text(available(state)).font(Theme.small).foregroundStyle(Theme.quiet)
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 8)
            .frame(maxWidth: .infinity, minHeight: 56, alignment: .leading)
            .overlay(RoundedRectangle(cornerRadius: 16).strokeBorder(Theme.line, style: StrokeStyle(lineWidth: 1, dash: [4, 3])))
            .accessibilityElement(children: .combine)
        }
    }

    private func status(_ connection: PluginConnection) -> String {
        switch connection.health {
        case .ok: connection.summary
        case .degraded: "Working, with problems"
        case .unreachable: "Can't reach it right now"
        case .credentialsRequired: "Needs you to sign in again"
        }
    }

    private func available(_ state: PluginState) -> String {
        if !state.available.isEmpty {
            return "\(state.available.map(\.name).joined(separator: ", ")). Connect on the web for now."
        }
        if state.connections.isEmpty { return "Your administrator hasn't offered any plugins yet." }
        return "\(state.connections.count == 2 ? "Both" : "All") connected. New plugins show up here to connect."
    }

    private func load() async {
        guard let api = model.api else { return }
        failed = false
        do {
            state = try await Plugins(api: api).load()
        } catch {
            if state == nil { failed = true }
            model.handle(error)
        }
    }
}

struct PluginTile: View {
    let name: String
    var size: CGFloat = 40

    var body: some View {
        let tone = PluginTone(name)
        Text(String(name.trimmingCharacters(in: .whitespaces).prefix(1)).uppercased())
            .font(.custom("Figtree-Bold", size: size == 40 ? 18 : 26))
            .foregroundStyle(tone.ink)
            .frame(width: size, height: size)
            .background(RoundedRectangle(cornerRadius: size == 40 ? 10 : 16).fill(tone.fill))
            .accessibilityHidden(true)
    }
}

struct PluginTone {
    let fill: Color
    let ink: Color

    private static let known: [String: (UInt32, UInt32)] = [
        "youtube": (0x5a2a4f, 0xe7a3c8),
        "spotify": (0x1f4a43, 0x8fd9c4),
    ]
    private static let palette: [(UInt32, UInt32)] = [
        (0x2a3a5c, 0xa9bcf0),
        (0x4a3a1f, 0xe8c98a),
        (0x3d2f5a, 0xc5b0ee),
        (0x3a4a22, 0xbfdc8a),
    ]

    init(_ name: String) {
        let key = name.trimmingCharacters(in: .whitespaces).lowercased().split(separator: " ").first.map(String.init) ?? ""
        let hash = key.unicodeScalars.reduce(UInt32(0)) { $0 &* 31 &+ $1.value }
        let pair = Self.known[key] ?? Self.palette[Int(hash % UInt32(Self.palette.count))]
        fill = Color(hex: pair.0)
        ink = Color(hex: pair.1)
    }
}

extension Color {
    init(hex: UInt32) {
        self.init(red: Double((hex >> 16) & 0xff) / 255, green: Double((hex >> 8) & 0xff) / 255, blue: Double(hex & 0xff) / 255)
    }
}
