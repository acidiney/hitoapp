import HitoKit
import SwiftUI

struct PlaylistView: View {
    enum Filter: Hashable {
        case all, ready, notReady
    }

    @Environment(AppModel.self) private var model
    let playlist: Playlist
    @State private var detail: PlaylistDetail?
    @State private var failed = false
    @State private var filter = Filter.all
    @State private var tracks: [Track] = []
    @State private var finding: (done: Int, total: Int)?
    @State private var findNote: String?
    @State private var search: Task<Void, Never>?

    var ready: [Track] { tracks.filter(\.isPlayable) }
    var notReady: [Track] { tracks.filter { !$0.isPlayable } }

    var shown: [Track] {
        switch filter {
        case .all: tracks
        case .ready: ready
        case .notReady: notReady
        }
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 0) {
                header
                VStack(alignment: .leading, spacing: 12) {
                    if failed {
                        HStack(spacing: 8) {
                            Text("This playlist didn't load.").foregroundStyle(Theme.soft)
                            TextAction(title: "Try again") { Task { await load() } }
                        }
                        .font(Theme.small)
                    } else if detail == nil {
                        ProgressView().tint(Theme.soft).frame(maxWidth: .infinity, minHeight: 60)
                    } else if tracks.isEmpty {
                        Text("No tracks in this playlist yet.").font(Theme.small).foregroundStyle(Theme.soft)
                    } else {
                        chips
                        VStack(spacing: 0) {
                            ForEach(shown) { track in
                                HStack(spacing: 0) {
                                    Button {
                                        model.player?.play(shown, at: shown.firstIndex(of: track) ?? 0, from: playlist.name)
                                    } label: {
                                        LibraryRow(track: track, isCurrent: model.player?.current?.id == track.id, showsTime: false)
                                    }
                                    .buttonStyle(.plain)
                                    .disabled(!track.isPlayable)
                                    Menu { TrackMenu(track: track) } label: {
                                        Icon("more-vertical").foregroundStyle(Theme.quiet).frame(width: 44, height: 44).contentShape(Circle())
                                    }
                                    .accessibilityLabel("More actions for \(track.title)")
                                }
                            }
                        }
                        .padding(.trailing, -8)
                    }
                }
                .padding(.horizontal, 16)
                .padding(.top, 14)
                .padding(.bottom, 16)
            }
        }
        .scrollIndicators(.hidden)
        .task { if detail == nil { await load() } }
        .onDisappear { search?.cancel() }
    }

    private var header: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack(alignment: .center, spacing: 14) {
                Cover(path: playlist.artworkPath, size: 96, radius: 10, mark: "playlist")
                VStack(alignment: .leading, spacing: 4) {
                    Text(detail?.origin ?? (playlist.kind == .followed ? "Followed playlist" : "Your playlist"))
                        .font(Theme.smallStrong)
                        .foregroundStyle(Theme.soft)
                        .lineLimit(2)
                    Text(playlist.name)
                        .font(Theme.title)
                        .lineLimit(3)
                        .accessibilityAddTraits(.isHeader)
                    if detail != nil {
                        Text("\(ready.count) of \(tracks.count) ready").foregroundStyle(Theme.soft)
                    }
                }
            }
            if !tracks.isEmpty {
                GeometryReader { proxy in
                    ZStack(alignment: .leading) {
                        Capsule().fill(Theme.rest)
                        Capsule().fill(Theme.text).frame(width: proxy.size.width * CGFloat(ready.count) / CGFloat(tracks.count))
                    }
                }
                .frame(height: 6)
                .accessibilityHidden(true)
            }
            if !tracks.isEmpty {
                HStack(spacing: 8) {
                    if !ready.isEmpty {
                        PlayAllButton(label: "Play \(ready.count) ready", stretch: true) {
                            model.player?.play(ready, at: 0, from: playlist.name)
                        }
                    }
                    if let finding {
                        OutlineAction(title: "Finding \(min(finding.done + 1, finding.total)) of \(finding.total)…", busy: true) { search?.cancel() }
                            .accessibilityHint("Stops looking for sources")
                    } else if !notReady.isEmpty {
                        OutlineAction(title: "Find \(notReady.count)", busy: false, action: findAll)
                            .accessibilityLabel("Find sources for \(notReady.count) tracks that aren't ready")
                    }
                    if !ready.isEmpty {
                        Menu {
                            Button("Play next") { ready.reversed().forEach { model.player?.playNext($0) } }
                        } label: {
                            Icon("more").foregroundStyle(Theme.text).frame(width: 48, height: 48).contentShape(Circle())
                        }
                        .accessibilityLabel("More actions")
                    }
                }
                if let findNote {
                    Text(findNote).font(Theme.small).foregroundStyle(Theme.soft)
                }
            }
        }
        .padding(.horizontal, 16)
        .padding(.top, 8)
        .padding(.bottom, 16)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Theme.raised.ignoresSafeArea(edges: .top))
    }

    private var chips: some View {
        HStack(spacing: 8) {
            FilterChip(title: "All \(tracks.count)", on: filter == .all) { filter = .all }
            FilterChip(title: "Ready \(ready.count)", on: filter == .ready) { filter = .ready }
            FilterChip(title: "Not ready \(tracks.count - ready.count)", on: filter == .notReady) { filter = .notReady }
        }
    }

    private func load() async {
        guard let api = model.api else { return }
        failed = false
        do {
            let loaded = try await Playlists(api: api).detail(playlist.id)
            detail = loaded
            tracks = loaded.tracks
        } catch {
            failed = true
            model.handle(error)
        }
    }

    private func findAll() {
        guard let api = model.api else { return }
        let ids = notReady.map(\.id)
        findNote = nil
        finding = (0, ids.count)
        search = Task {
            do {
                let found = try await Playback(api: api).find(ids) { id, result in
                    if result == .found, let index = tracks.firstIndex(where: { $0.id == id }) {
                        tracks[index] = tracks[index].readied
                    }
                    finding?.done += 1
                }
                findNote = found == 0 ? "No sources found. These tracks stay No source." : "Found sources for \(found) of \(ids.count)."
            } catch is CancellationError {
                findNote = "Stopped looking. Tracks found so far are ready."
            } catch {
                findNote = "The search stopped. Try again."
                model.handle(error)
            }
            finding = nil
        }
    }
}

struct OutlineAction: View {
    let title: String
    let busy: Bool
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            HStack(spacing: 8) {
                if busy {
                    ProgressView().tint(Theme.soft).controlSize(.small)
                } else {
                    Circle().strokeBorder(Theme.warning, lineWidth: 1.5).frame(width: 10, height: 10)
                }
                Text(title).font(Theme.bodyStrong).lineLimit(1).minimumScaleFactor(0.8)
            }
            .foregroundStyle(Theme.text)
            .padding(.horizontal, 16)
            .frame(maxWidth: .infinity, minHeight: 48)
            .overlay(Capsule().strokeBorder(Theme.line))
            .contentShape(Capsule())
        }
        .buttonStyle(.plain)
    }
}

struct FilterChip: View {
    let title: String
    let on: Bool
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Text(title)
                .font(Theme.bodyMedium)
                .foregroundStyle(on ? Theme.ground : Theme.text)
                .padding(.horizontal, 16)
                .frame(minHeight: 44)
                .background(Capsule().fill(on ? Theme.text : .clear))
                .overlay(Capsule().strokeBorder(on ? .clear : Theme.line))
        }
        .buttonStyle(.plain)
        .accessibilityAddTraits(on ? .isSelected : [])
    }
}
