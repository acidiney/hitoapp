import HitoKit
import SwiftUI

struct SearchView: View {
    @Environment(AppModel.self) private var model
    @State private var text = ""
    @State private var tracks = Feed<Track>()
    @State private var artist: Artist?
    @State private var searchable: [PluginConnection] = []
    @State private var scope: PluginConnection?
    @State private var remote: [PluginResult] = []
    @State private var remoteState = RemoteState.idle
    @State private var adopting: String?
    @State private var adoptNote: String?
    @State private var importing: String?
    @State private var opened: Playlist?
    @FocusState private var focused: Bool

    enum RemoteState: Equatable {
        case idle, searching, failed
    }

    var query: String {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        return PlaylistLink.looksLikePlaylist(trimmed) ? "" : trimmed
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack(spacing: 10) {
                Icon("search").foregroundStyle(Theme.quiet)
                TextField("", text: $text, prompt: Text("Search, or paste a playlist link").foregroundStyle(Theme.quiet))
                    .accessibilityLabel("Search")
                    .focused($focused)
                    .submitLabel(.search)
                    .onSubmit { focused = false }
                    .autocorrectionDisabled()
                    .textInputAutocapitalization(.never)
                if !text.isEmpty {
                    IconButton(icon: "close", label: "Clear search", color: Theme.quiet) { text = "" }
                }
            }
            .padding(.leading, 16)
            .padding(.trailing, 4)
            .frame(minHeight: 48)
            .background(Capsule().fill(Theme.field))
            .overlay(Capsule().strokeBorder(focused ? Theme.accent : Theme.line, lineWidth: focused ? 2 : 1))
            .padding(.horizontal, 16)
            if PlaylistLink.looksLikePlaylist(text) {
                HStack(spacing: 12) {
                    Icon("link", size: 20).foregroundStyle(Theme.focus)
                    VStack(alignment: .leading, spacing: 0) {
                        Text("\(PlaylistLink.service(of: text) ?? "Playlist") playlist link").font(Theme.bodyStrong)
                        Text("Follow it or copy it").font(Theme.small).foregroundStyle(Theme.quiet)
                    }
                    Spacer(minLength: 8)
                    Button { importing = text.trimmingCharacters(in: .whitespacesAndNewlines) } label: {
                        Text("Open").font(Theme.smallStrong).foregroundStyle(Theme.onAccent).padding(.horizontal, 18).frame(minHeight: 40).background(Capsule().fill(Theme.accent))
                    }
                    .buttonStyle(.plain)
                }
                .padding(12)
                .background(RoundedRectangle(cornerRadius: 14).fill(Theme.field))
                .padding(.horizontal, 16)
                .accessibilityElement(children: .combine)
            }
            if !searchable.isEmpty {
                ScrollView(.horizontal) {
                    HStack(spacing: 8) {
                        Text("Your library")
                            .font(Theme.bodyMedium)
                            .padding(.horizontal, 16)
                            .frame(minHeight: 44)
                            .background(Capsule().fill(Theme.field))
                        ForEach(searchable) { plugin in
                            let on = scope?.id == plugin.id
                            Button { scope = on ? nil : plugin } label: {
                                HStack(spacing: 8) {
                                    if on { Icon("check", size: 16).foregroundStyle(Theme.accent) }
                                    Text(plugin.name).font(Theme.bodyMedium)
                                }
                                .foregroundStyle(Theme.text)
                                .padding(.horizontal, 16)
                                .frame(minHeight: 44)
                                .overlay(Capsule().strokeBorder(on ? Theme.accent : Theme.line, lineWidth: on ? 1.5 : 1))
                            }
                            .buttonStyle(.plain)
                            .accessibilityLabel("Also search \(plugin.name)")
                            .accessibilityAddTraits(on ? .isSelected : [])
                        }
                    }
                    .padding(.horizontal, 16)
                }
                .scrollIndicators(.hidden)
            }
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 14) {
                    if query.isEmpty {
                        Text("Find tracks and artists in your library.")
                            .font(Theme.small)
                            .foregroundStyle(Theme.soft)
                    } else if tracks.isEmpty && artist == nil {
                        VStack(alignment: .leading, spacing: 6) {
                            Text("Nothing in your library matches “\(query)”").font(Theme.bodyStrong)
                            Text("Check the spelling, or try fewer words.").font(Theme.small).foregroundStyle(Theme.soft)
                        }
                    } else {
                        if let artist { TopResult(artist: artist) }
                        if !tracks.items.isEmpty {
                            Text("Tracks").font(Theme.heading).padding(.top, 4).accessibilityAddTraits(.isHeader)
                        }
                        VStack(spacing: 0) {
                            ForEach(Array(tracks.items.enumerated()), id: \.element.id) { index, track in
                                HStack(spacing: 0) {
                                    Button {
                                        model.player?.play(tracks.items, at: index, from: "Search")
                                    } label: {
                                        SearchRow(track: track, isCurrent: model.player?.current?.id == track.id)
                                    }
                                    .buttonStyle(.plain)
                                    .disabled(!track.isPlayable)
                                    if !track.isPlayable {
                                        FindButton(track: track) { tracks.replace(track.id, with: $0) }
                                    }
                                    Menu { TrackMenu(track: track) } label: {
                                        Icon("more-vertical").foregroundStyle(Theme.quiet).frame(width: 44, height: 44).contentShape(Circle())
                                    }
                                    .accessibilityLabel("More actions for \(track.title)")
                                }
                            }
                            FeedFooter(feed: tracks)
                        }
                        .padding(.trailing, -8)
                    }
                    if let scope, !query.isEmpty {
                        remoteSection(scope)
                    }
                }
                .padding(.horizontal, 16)
                .padding(.bottom, 16)
            }
            .scrollIndicators(.hidden)
        }
        .task(id: query) {
            guard !query.isEmpty, let api = model.api else {
                artist = nil
                return
            }
            try? await Task.sleep(for: .milliseconds(250))
            guard !Task.isCancelled else { return }
            let library = Library(api: api)
            let search = query
            tracks.reset { try await library.tracks(offset: $0, search: search) }
            async let found = try? library.artists(offset: 0, search: search).items.first
            await tracks.more(model)
            let match = await found
            guard !Task.isCancelled else { return }
            artist = match
        }
        .task(id: "\(query)|\(scope?.id ?? "")") { await searchRemote() }
        .task { await loadSearchable() }
        .fullScreenCover(item: Binding(get: { importing.map(Link.init) }, set: { importing = $0?.value })) { link in
            ImportSheet(initialLink: link.value) { id in
                Task {
                    guard let api = model.api else { return }
                    opened = (try? await Playlists(api: api).list())?.first { $0.id == id }
                }
            }
            .presentationBackground(.clear)
        }
        .navigationDestination(item: $opened) { PlaylistView(playlist: $0).pushed() }
        .onAppear { focused = true }
    }

    private func remoteSection(_ plugin: PluginConnection) -> some View {
        VStack(alignment: .leading, spacing: 0) {
            Text("From \(plugin.name)").font(Theme.heading).padding(.top, 8).padding(.bottom, 4).accessibilityAddTraits(.isHeader)
            switch remoteState {
            case .searching where remote.isEmpty:
                Text("Searching \(plugin.name)…").font(Theme.small).foregroundStyle(Theme.soft).padding(.vertical, 8)
            case .failed:
                Text("\(plugin.name) couldn't be searched. Your library results are unaffected.").font(Theme.small).foregroundStyle(Theme.alert).padding(.vertical, 8)
            default:
                if remote.isEmpty {
                    Text("Nothing on \(plugin.name) matches “\(query)”.").font(Theme.small).foregroundStyle(Theme.soft).padding(.vertical, 8)
                }
            }
            if let adoptNote {
                Text(adoptNote).font(Theme.small).foregroundStyle(Theme.soft).padding(.vertical, 6)
            }
            ForEach(remote) { result in
                HStack(spacing: 12) {
                    ZStack { Theme.tile }.frame(width: 44, height: 44).clipShape(RoundedRectangle(cornerRadius: 6)).accessibilityHidden(true)
                    VStack(alignment: .leading, spacing: 0) {
                        Text(result.title).font(Theme.bodyStrong).lineLimit(1)
                        Text(([result.artist] + (result.durationMs.map { [Duration.milliseconds($0).clock] } ?? [])).joined(separator: " · ")).font(Theme.small).foregroundStyle(Theme.quiet).lineLimit(1)
                    }
                    Spacer(minLength: 4)
                    if plugin.capabilities.contains("sources") {
                        RoundAction(icon: "play", label: "Play \(result.title)", busy: adopting == result.id) { Task { await adopt(result, from: plugin, play: true) } }
                    }
                    RoundAction(icon: "plus", label: "Add \(result.title) to your library", busy: adopting == result.id) { Task { await adopt(result, from: plugin, play: false) } }
                }
                .frame(minHeight: 60)
                .accessibilityElement(children: .contain)
            }
        }
    }

    private func loadSearchable() async {
        guard let api = model.api, let state = try? await Plugins(api: api).load() else { return }
        searchable = state.connections.filter { $0.capabilities.contains("search") }
        if let scope, !searchable.contains(where: { $0.id == scope.id }) { self.scope = nil }
    }

    private func searchRemote() async {
        remote = []
        adoptNote = nil
        guard let scope, !query.isEmpty, let api = model.api else {
            remoteState = .idle
            return
        }
        remoteState = .searching
        try? await Task.sleep(for: .milliseconds(350))
        guard !Task.isCancelled else { return }
        do {
            let found = try await Plugins(api: api).search(scope.id, revision: scope.revision, query: query)
            guard !Task.isCancelled else { return }
            remote = found
            remoteState = .idle
        } catch is CancellationError {
        } catch {
            guard !Task.isCancelled else { return }
            remoteState = .failed
            model.handle(error)
        }
    }

    private func adopt(_ result: PluginResult, from plugin: PluginConnection, play: Bool) async {
        guard let api = model.api else { return }
        adopting = result.id
        defer { adopting = nil }
        do {
            let track = try await Plugins(api: api).adopt(plugin.id, revision: plugin.revision, result: result.id)
            remote.removeAll { $0.id == result.id }
            if play && track.available {
                model.player?.play([track], at: 0, from: "Search")
                adoptNote = nil
            } else {
                adoptNote = play ? "No source is ready to play \(track.title) yet. It was added to your library." : "Added \(track.title) to your library."
            }
        } catch {
            model.handle(error)
        }
    }
}

struct RoundAction: View {
    let icon: String
    let label: String
    var busy = false
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Group {
                if busy { ProgressView().tint(Theme.soft) } else { Icon(icon, size: 16) }
            }
            .foregroundStyle(Theme.text)
            .frame(width: 44, height: 44)
            .overlay(Circle().strokeBorder(Theme.line))
        }
        .buttonStyle(.plain)
        .disabled(busy)
        .accessibilityLabel(label)
    }
}

struct TopResult: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dynamicTypeSize) private var typeSize
    let artist: Artist

    var body: some View {
        HStack(spacing: 14) {
            ZStack {
                Theme.tile
                Icon("person", size: 28).foregroundStyle(Theme.coverMark)
            }
            .frame(width: 64, height: 64)
            .clipShape(Circle())
            .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 0) {
                Text("Top result").font(Theme.label).foregroundStyle(Theme.focus)
                Text(artist.name).font(Theme.result).lineLimit(typeSize.isAccessibilitySize ? 3 : 1)
                Text("Artist · \(artist.trackCount) \(artist.trackCount == 1 ? "track" : "tracks") in your library")
                    .font(Theme.small)
                    .foregroundStyle(Theme.quiet)
                    .lineLimit(typeSize.isAccessibilitySize ? 3 : 1)
            }
            Spacer(minLength: 0)
            Button(action: play) {
                Icon("play", size: 16)
                    .foregroundStyle(Theme.onAccent)
                    .frame(width: 44, height: 44)
                    .background(Circle().fill(Theme.accent))
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Play \(artist.name)")
        }
        .padding(12)
        .background(RoundedRectangle(cornerRadius: 16).fill(Theme.field))
    }

    private func play() {
        guard let api = model.api else { return }
        let id = artist.id
        let name = artist.name
        Task {
            do {
                let page = try await Library(api: api).tracks(offset: 0, artist: id)
                model.player?.play(page.items, at: page.items.firstIndex { $0.isPlayable } ?? 0, from: name)
            } catch {
                model.handle(error)
            }
        }
    }
}

struct Link: Identifiable {
    let value: String
    var id: String { value }
}
