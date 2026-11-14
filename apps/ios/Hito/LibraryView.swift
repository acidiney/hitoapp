import HitoKit
import SwiftUI

struct LibraryView: View {
    enum Section: String, CaseIterable {
        case tracks = "Tracks"
        case albums = "Albums"
    }

    @Environment(AppModel.self) private var model
    @State private var section = Section.tracks
    @State private var tracks = Feed<Track>()
    @State private var albums = Feed<Album>()

    var body: some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 12) {
                Text("Your library").font(Theme.title).accessibilityAddTraits(.isHeader)
                SectionTabs(selection: $section)
                switch section {
                case .tracks:
                    TrackRows(feed: tracks, context: "Your library", empty: "No tracks yet. An administrator adds music folders on the web.")
                        .padding(.leading, -6)
                        .padding(.trailing, -8)
                case .albums:
                    AlbumGrid(feed: albums)
                }
            }
            .padding(.horizontal, 16)
            .padding(.bottom, 16)
        }
        .scrollIndicators(.hidden)
        .refreshable { reload() }
        .task {
            if tracks.total == nil && albums.total == nil { reload() }
        }
    }

    private func reload() {
        guard let api = model.api else { return }
        let library = Library(api: api)
        tracks.reset { try await library.tracks(offset: $0) }
        albums.reset { try await library.albums(offset: $0) }
    }
}

struct SectionTabs: View {
    @Binding var selection: LibraryView.Section

    var body: some View {
        HStack(spacing: 4) {
            ForEach(LibraryView.Section.allCases, id: \.self) { item in
                let current = item == selection
                Button { selection = item } label: {
                    Text(item.rawValue)
                        .font(current ? Theme.bodyBold : Theme.bodyMedium)
                        .foregroundStyle(current ? Theme.text : Theme.quiet)
                        .padding(.horizontal, 12)
                        .frame(minHeight: 44)
                        .overlay(alignment: .bottom) {
                            (current ? Theme.accent : .clear).frame(height: 2)
                        }
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityAddTraits(current ? .isSelected : [])
            }
            Spacer()
        }
        .padding(.leading, 16)
        .overlay(alignment: .bottom) { Theme.barLine.frame(height: 1) }
        .padding(.horizontal, -16)
    }
}

struct TrackRows: View {
    @Environment(AppModel.self) private var model
    let feed: Feed<Track>
    let context: String
    let empty: String

    var body: some View {
        if feed.isEmpty {
            Text(empty).font(Theme.small).foregroundStyle(Theme.soft).padding(.vertical, 12)
        }
        ForEach(Array(feed.items.enumerated()), id: \.element.id) { index, track in
            Button {
                model.player?.play(feed.items, at: index, from: context)
            } label: {
                LibraryRow(track: track, isCurrent: model.player?.current?.id == track.id)
            }
            .buttonStyle(.plain)
            .disabled(!track.isPlayable)
            .contextMenu { TrackMenu(track: track) }
        }
        FeedFooter(feed: feed)
    }
}

struct TrackMenu: View {
    @Environment(AppModel.self) private var model
    let track: Track

    var body: some View {
        if track.isPlayable {
            Button("Play next") { model.player?.playNext(track) }
        }
        if let player = model.player {
            Button(player.isLiked(track) ? "Remove from Liked tracks" : "Like") { player.toggleLike(track) }
        }
    }
}

struct AlbumGrid: View {
    let feed: Feed<Album>

    var body: some View {
        if feed.isEmpty {
            Text("No albums yet.").font(Theme.small).foregroundStyle(Theme.soft).padding(.vertical, 12)
        }
        LazyVGrid(columns: [GridItem(.flexible(), spacing: 12), GridItem(.flexible(), spacing: 12)], spacing: 16) {
            ForEach(feed.items) { album in
                NavigationLink(value: album) { AlbumCard(album: album) }
                    .buttonStyle(.plain)
            }
        }
        FeedFooter(feed: feed)
    }
}

struct TrackListView: View {
    @Environment(AppModel.self) private var model
    let list: TrackList
    @State private var feed = Feed<Track>()

    var body: some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 14) {
                Text(list.title).font(Theme.title).accessibilityAddTraits(.isHeader)
                if let total = feed.total, total > 0 {
                    PlayAllButton(label: "Play") {
                        model.player?.play(feed.items, at: feed.items.firstIndex { $0.isPlayable } ?? 0, from: list.title)
                    }
                }
                VStack(spacing: 0) {
                    TrackRows(feed: feed, context: list.title, empty: "Tracks you like show up here.")
                }
                .padding(.leading, -6)
                .padding(.trailing, -8)
            }
            .padding(.horizontal, 16)
            .padding(.bottom, 16)
        }
        .scrollIndicators(.hidden)
        .task {
            guard feed.total == nil, let api = model.api else { return }
            let library = Library(api: api)
            feed.reset { try await library.tracks(offset: $0, liked: true) }
        }
    }
}

struct PlayAllButton: View {
    let label: String
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            HStack(spacing: 8) {
                Icon("play", size: 16)
                Text(label).font(Theme.bodyStrong)
            }
            .foregroundStyle(Theme.onAccent)
            .padding(.horizontal, 22)
            .frame(minHeight: 48)
            .background(Capsule().fill(Theme.accent))
        }
        .buttonStyle(.plain)
    }
}

struct AlbumView: View {
    @Environment(AppModel.self) private var model
    let album: Album
    @State private var detail: AlbumDetail?
    @State private var failed = false

    var tracks: [Track] { detail?.tracks ?? [] }

    var body: some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 14) {
                HStack(alignment: .top, spacing: 14) {
                    Cover(path: album.artworkPath, size: 96, radius: 10)
                    VStack(alignment: .leading, spacing: 4) {
                        Text("Album").font(Theme.small).foregroundStyle(Theme.soft)
                        Text(album.title)
                            .font(Theme.pageTitle)
                            .lineLimit(3)
                            .accessibilityAddTraits(.isHeader)
                        Text(facts).font(Theme.small).foregroundStyle(Theme.quiet)
                    }
                }
                HStack(spacing: 8) {
                    if tracks.contains(where: \.isPlayable) {
                        PlayAllButton(label: "Play") {
                            model.player?.play(tracks, at: tracks.firstIndex { $0.isPlayable } ?? 0, from: album.title)
                        }
                        Menu {
                            Button("Play next") { tracks.filter(\.isPlayable).reversed().forEach { model.player?.playNext($0) } }
                        } label: {
                            Icon("more").foregroundStyle(Theme.text).frame(width: 48, height: 48).contentShape(Circle())
                        }
                        .accessibilityLabel("More actions")
                    }
                }
                if failed {
                    HStack(spacing: 8) {
                        Text("This album didn't load.").foregroundStyle(Theme.soft)
                        TextAction(title: "Try again") { Task { await load() } }
                    }
                    .font(Theme.small)
                } else if detail == nil {
                    ProgressView().tint(Theme.soft).frame(maxWidth: .infinity, minHeight: 60)
                }
                VStack(spacing: 0) {
                    ForEach(Array(tracks.enumerated()), id: \.element.id) { index, track in
                        HStack(spacing: 4) {
                            Button {
                                model.player?.play(tracks, at: index, from: album.title)
                            } label: {
                                AlbumTrackRow(track: track, isCurrent: model.player?.current?.id == track.id)
                            }
                            .buttonStyle(.plain)
                            .disabled(!track.isPlayable)
                            Icon("check", size: 16)
                                .foregroundStyle(Theme.focus)
                                .frame(width: 44, height: 44)
                                .accessibilityLabel("In your library")
                            Menu { TrackMenu(track: track) } label: {
                                Icon("more").foregroundStyle(Theme.quiet).frame(width: 44, height: 44).contentShape(Circle())
                            }
                            .accessibilityLabel("More actions for \(track.title)")
                        }
                    }
                }
                .padding(.trailing, -8)
            }
            .padding(.horizontal, 16)
            .padding(.bottom, 16)
        }
        .scrollIndicators(.hidden)
        .task { if detail == nil { await load() } }
    }

    private var facts: String {
        let count = "\(album.trackCount) \(album.trackCount == 1 ? "track" : "tracks")"
        return [album.artist ?? "Various artists", count].joined(separator: " · ")
    }

    private func load() async {
        guard let api = model.api else { return }
        failed = false
        do {
            detail = try await Library(api: api).album(album.id)
        } catch {
            failed = true
            model.handle(error)
        }
    }
}
