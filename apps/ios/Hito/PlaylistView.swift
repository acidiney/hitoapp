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

    var tracks: [Track] { detail?.tracks ?? [] }
    var ready: [Track] { tracks.filter(\.isPlayable) }

    var shown: [Track] {
        switch filter {
        case .all: tracks
        case .ready: ready
        case .notReady: tracks.filter { !$0.isPlayable }
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
                    if let detail {
                        Text("\(detail.ready) of \(tracks.count) ready").foregroundStyle(Theme.soft)
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
            if !ready.isEmpty {
                HStack(spacing: 8) {
                    PlayAllButton(label: "Play \(ready.count) ready") {
                        model.player?.play(ready, at: 0, from: playlist.name)
                    }
                    Menu {
                        Button("Play next") { ready.reversed().forEach { model.player?.playNext($0) } }
                    } label: {
                        Icon("more").foregroundStyle(Theme.text).frame(width: 48, height: 48).contentShape(Circle())
                    }
                    .accessibilityLabel("More actions")
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
            detail = try await Playlists(api: api).detail(playlist.id)
        } catch {
            failed = true
            model.handle(error)
        }
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
