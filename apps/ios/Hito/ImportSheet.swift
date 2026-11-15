import HitoKit
import SwiftUI

struct ImportSheet: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    var initialLink = ""
    let added: (String) -> Void
    @State private var link = ""
    @State private var connections: [PluginConnection]?
    @State private var following = true
    @State private var interval = FollowInterval.daily
    @State private var working = false
    @State private var problem: String?
    @State private var request = UUID()

    private var connection: PluginConnection? {
        connections.flatMap { PlaylistLink.connection(for: link, among: $0) }
    }

    private var service: String {
        PlaylistLink.service(of: link) ?? connection?.name ?? "the service"
    }

    var body: some View {
        VStack(spacing: 0) {
            Color.black.opacity(0.55)
                .onTapGesture { dismiss() }
                .accessibilityHidden(true)
            sheet
        }
        .ignoresSafeArea(edges: .top)
        .task { if link.isEmpty { link = initialLink }; await load() }
        .onChange(of: link) { request = UUID(); problem = nil }
        .onChange(of: following) { request = UUID() }
    }

    private var sheet: some View {
        VStack(alignment: .leading, spacing: 16) {
            Capsule().fill(Theme.line).frame(width: 40, height: 4).frame(maxWidth: .infinity)
            Text("Import a playlist").font(Theme.playing).accessibilityAddTraits(.isHeader)
            VStack(alignment: .leading, spacing: 6) {
                Text("Playlist link").font(Theme.small).foregroundStyle(Theme.quiet)
                HStack(spacing: 10) {
                    Icon("link", size: 18).foregroundStyle(Theme.focus)
                    TextField("", text: $link, prompt: Text("Paste a Spotify or YouTube playlist link").foregroundStyle(Theme.quiet))
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                        .keyboardType(.URL)
                        .submitLabel(.done)
                }
                .padding(.horizontal, 14)
                .frame(minHeight: 48)
                .background(RoundedRectangle(cornerRadius: 10).fill(Theme.ground))
                .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(Theme.line))
            }
            source
            if connection != nil, !link.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                VStack(alignment: .leading, spacing: 10) {
                    Text("How to add it").font(Theme.small).foregroundStyle(Theme.quiet).padding(.bottom, -2)
                    ModeCard(title: "Follow", detail: "Stays the same as on \(service). You can't edit it.", selected: following) { following = true }
                    ModeCard(title: "Copy", detail: "Yours to edit. It won't change when \(service) does.", selected: !following) { following = false }
                }
                if following {
                    HStack(spacing: 10) {
                        Text("While you follow it").font(Theme.small).foregroundStyle(Theme.quiet)
                        Menu {
                            ForEach(FollowInterval.allCases, id: \.self) { every in
                                Button(every.label) { interval = every }
                            }
                        } label: {
                            HStack(spacing: 8) {
                                Text("Check").foregroundStyle(Theme.quiet)
                                Text(interval.label).font(Theme.smallMedium)
                                Icon("chevron-down", size: 14)
                            }
                            .font(Theme.small)
                            .foregroundStyle(Theme.text)
                            .padding(.leading, 16)
                            .padding(.trailing, 12)
                            .frame(minHeight: 44)
                            .overlay(Capsule().strokeBorder(Theme.line))
                        }
                    }
                }
            }
            if let problem {
                Text(problem).font(Theme.small).foregroundStyle(Color(hex: 0xf6d3ca))
                    .padding(12)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(RoundedRectangle(cornerRadius: 10).fill(Color(hex: 0x2a1b17)))
            }
            Button { Task { await add() } } label: {
                Text(working ? (following ? "Following…" : "Copying…") : (following ? "Follow playlist" : "Copy playlist"))
                    .font(Theme.bodyStrong)
                    .foregroundStyle(Theme.onAccent)
                    .frame(maxWidth: .infinity, minHeight: 48)
                    .background(Capsule().fill(Theme.accent))
                    .opacity(ready ? 1 : 0.45)
            }
            .buttonStyle(.plain)
            .disabled(!ready)
            Button { dismiss() } label: {
                Text("Cancel")
                    .font(Theme.bodyMedium)
                    .foregroundStyle(Theme.text)
                    .frame(maxWidth: .infinity, minHeight: 48)
                    .overlay(Capsule().strokeBorder(Theme.line))
            }
            .buttonStyle(.plain)
        }
        .padding(.horizontal, 16)
        .padding(.top, 8)
        .padding(.bottom, 28)
        .foregroundStyle(Theme.text)
        .background(UnevenRoundedRectangle(topLeadingRadius: 16, topTrailingRadius: 16).fill(Theme.field).ignoresSafeArea(edges: .bottom))
        .accessibilityAddTraits(.isModal)
    }

    @ViewBuilder private var source: some View {
        let trimmed = link.trimmingCharacters(in: .whitespacesAndNewlines)
        if let connections {
            if connections.allSatisfy({ !$0.capabilities.contains("playlists") }) {
                note("No connected plugin can add playlists. Connect one in Plugins, then try again.")
            } else if let connection, !trimmed.isEmpty {
                HStack(spacing: 14) {
                    ZStack { Theme.tile; Icon("playlist", size: 24).foregroundStyle(Theme.coverMark) }
                        .frame(width: 56, height: 56)
                        .clipShape(RoundedRectangle(cornerRadius: 6))
                        .accessibilityHidden(true)
                    VStack(alignment: .leading, spacing: 2) {
                        Text("\(service) playlist").font(Theme.bodyStrong)
                        Text("Through \(connection.name)").font(Theme.small).foregroundStyle(Theme.quiet)
                    }
                    Spacer(minLength: 0)
                }
                .padding(12)
                .background(RoundedRectangle(cornerRadius: 10).fill(Theme.raised))
                .accessibilityElement(children: .combine)
            } else if !trimmed.isEmpty {
                note("No connected plugin matches this link. Use a link from a service you've connected.")
            }
        } else {
            ProgressView().tint(Theme.soft).frame(maxWidth: .infinity)
        }
    }

    private func note(_ text: String) -> some View {
        Text(text).font(Theme.small).foregroundStyle(Theme.soft)
    }

    private var ready: Bool {
        !working && connection != nil && !link.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    private func load() async {
        guard let api = model.api else { return }
        do {
            connections = try await Plugins(api: api).load().connections
        } catch {
            connections = []
            problem = "Hito couldn't load your plugins. Try again."
            model.handle(error)
        }
    }

    private func add() async {
        guard let api = model.api, let connection else { return }
        working = true
        problem = nil
        defer { working = false }
        do {
            let id = try await Playlists(api: api).add(link, from: connection, as: following ? .follow(interval) : .copy, request: request)
            added(id)
            dismiss()
        } catch {
            problem = PlaylistLink.problem(error, service: service, following: following)
            if case HitoAPI.Problem.signedOut = error { model.handle(error) }
            if case HitoAPI.Problem.locked = error { model.handle(error) }
        }
    }
}

struct ModeCard: View {
    let title: String
    let detail: String
    let selected: Bool
    let choose: () -> Void

    var body: some View {
        Button(action: choose) {
            HStack(alignment: .top, spacing: 12) {
                ZStack {
                    Circle().strokeBorder(selected ? Theme.accent : Theme.quiet, lineWidth: 1.5)
                    if selected { Circle().fill(Theme.accent).padding(4) }
                }
                .frame(width: 16, height: 16)
                .padding(.top, 4)
                VStack(alignment: .leading, spacing: 2) {
                    Text(title).font(Theme.bodyStrong)
                    Text(detail).font(Theme.small).foregroundStyle(Theme.soft).fixedSize(horizontal: false, vertical: true)
                }
                Spacer(minLength: 0)
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 14)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(RoundedRectangle(cornerRadius: 10).fill(selected ? Theme.chosen : .clear))
            .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(selected ? Theme.accent : Theme.line, lineWidth: 2))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityAddTraits(selected ? [.isButton, .isSelected] : .isButton)
    }
}
