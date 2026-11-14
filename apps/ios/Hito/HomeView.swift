import HitoKit
import SwiftUI

struct HomeView: View {
    @Environment(AppModel.self) private var model
    let user: User
    let openSearch: () -> Void
    @State private var albums = Feed<Album>()
    @State private var summary: HomeSummary?
    @State private var playlists: [Playlist] = []

    var name: String { user.displayName ?? user.username }

    var greeting: String {
        switch Calendar.current.component(.hour, from: .now) {
        case 5..<12: "Good morning"
        case 12..<18: "Good afternoon"
        default: "Good evening"
        }
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                HStack(spacing: 12) {
                    Text("\(greeting), \(name)")
                        .font(Theme.title)
                        .accessibilityAddTraits(.isHeader)
                    Spacer(minLength: 0)
                    NavigationLink(value: SettingsPage.root) {
                        AccountPicture(user: user)
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("Settings for \(name)")
                }
                Button(action: openSearch) {
                    HStack(spacing: 10) {
                        Icon("search")
                        Text("Search, or paste a playlist link")
                        Spacer()
                    }
                    .foregroundStyle(Theme.quiet)
                    .padding(.horizontal, 16)
                    .frame(minHeight: 48)
                    .background(Capsule().fill(Theme.field))
                    .overlay(Capsule().strokeBorder(Theme.line))
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Search")
                if let summary {
                    JumpBackIn(tiles: summary.tiles(playlists: playlists))
                    ForEach(summary.unheard.prefix(1)) { UnheardCard(album: $0) }
                } else if !albums.isEmpty {
                    JumpBackIn(tiles: [.liked(0)] + albums.items.prefix(5).map(QuickTile.album))
                }
                RecentlyAdded(feed: albums)
            }
            .padding(.horizontal, 16)
            .padding(.bottom, 16)
        }
        .scrollIndicators(.hidden)
        .refreshable {
            reload()
            await albums.more(model)
            await loadSummary()
        }
        .task {
            guard albums.total == nil else { return }
            reload()
            await albums.more(model)
            await loadSummary()
        }
    }

    private func loadSummary() async {
        guard let api = model.api else { return }
        summary = try? await HomeFeed(api: api).load()
        if summary != nil { playlists = (try? await Playlists(api: api).list()) ?? [] }
    }

    private func reload() {
        guard let api = model.api else { return }
        let library = Library(api: api)
        albums.reset { try await library.albums(offset: $0) }
    }
}

struct JumpBackIn: View {
    @Environment(\.dynamicTypeSize) private var typeSize
    let tiles: [QuickTile]

    var body: some View {
        LazyVGrid(columns: Array(repeating: GridItem(.flexible(), spacing: 8), count: typeSize.isAccessibilitySize ? 1 : 2), spacing: 8) {
            ForEach(tiles) { tile in
                switch tile {
                case .liked:
                    NavigationLink(value: TrackList.liked) {
                        Tile(title: tile.title) {
                            ZStack {
                                Theme.accent
                                Icon("heart-filled", size: 24.6).foregroundStyle(Theme.onAccent)
                            }
                            .frame(width: 56, height: 56)
                        }
                    }
                case let .album(album):
                    NavigationLink(value: album) {
                        Tile(title: album.title) { Cover(path: album.artworkPath, size: 56, radius: 0) }
                    }
                case let .playlist(playlist):
                    NavigationLink(value: playlist) {
                        Tile(title: playlist.name) {
                            ZStack { Theme.tile; Icon("playlist", size: 22).foregroundStyle(Theme.quiet) }
                                .frame(width: 56, height: 56)
                        }
                    }
                }
            }
        }
        .buttonStyle(.plain)
        .accessibilityLabel("Jump back in")
    }
}

struct UnheardCard: View {
    @Environment(AppModel.self) private var model
    let album: UnheardAlbum

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(alignment: .top, spacing: 14) {
                Cover(path: album.album.artworkPath, size: 72, radius: 8)
                VStack(alignment: .leading, spacing: 4) {
                    Text("Unheard, near what you like").font(Theme.smallStrong).foregroundStyle(Theme.focus)
                    Text(album.headline).font(Theme.bodyStrong).fixedSize(horizontal: false, vertical: true)
                    Text(album.reason).font(Theme.small).foregroundStyle(Theme.soft).fixedSize(horizontal: false, vertical: true)
                }
            }
            HStack(spacing: 8) {
                PlayAllButton(label: "Listen") {
                    let ready = album.tracks.filter(\.isPlayable)
                    if !ready.isEmpty { model.player?.play(ready, at: 0, from: album.title) }
                }
                .accessibilityLabel("Listen to \(album.title)")
                NavigationLink(value: album.album) {
                    Text("Open album").font(Theme.bodyMedium).foregroundStyle(Theme.text)
                        .padding(.horizontal, 18).frame(minHeight: 48)
                        .overlay(Capsule().strokeBorder(Theme.line))
                }
                .buttonStyle(.plain)
            }
        }
        .padding(14)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: 16).fill(Theme.field))
    }
}

struct Tile<Art: View>: View {
    let title: String
    @ViewBuilder let art: Art

    var body: some View {
        HStack(spacing: 10) {
            art
            Text(title)
                .font(Theme.smallStrong)
                .lineLimit(2)
                .multilineTextAlignment(.leading)
                .padding(.trailing, 10)
            Spacer(minLength: 0)
        }
        .frame(height: 56)
        .background(Theme.field)
        .clipShape(RoundedRectangle(cornerRadius: 10))
    }
}

struct RecentlyAdded: View {
    let feed: Feed<Album>

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Recently added").font(Theme.section).accessibilityAddTraits(.isHeader)
            if feed.isEmpty {
                Text("Albums you add show up here. An administrator adds music folders on the web.")
                    .font(Theme.small)
                    .foregroundStyle(Theme.soft)
            } else if feed.failed && feed.items.isEmpty {
                FeedFooter(feed: feed)
            } else if feed.items.isEmpty {
                HStack(spacing: 12) {
                    ForEach(0..<3, id: \.self) { _ in
                        RoundedRectangle(cornerRadius: 10).fill(Theme.field).frame(width: 136, height: 136)
                    }
                }
                .accessibilityLabel("Loading recently added albums")
            } else {
                ScrollView(.horizontal) {
                    LazyHStack(alignment: .top, spacing: 12) {
                        ForEach(feed.items.prefix(20)) { album in
                            NavigationLink(value: album) { AlbumCard(album: album, width: 136) }
                                .buttonStyle(.plain)
                        }
                    }
                }
                .scrollIndicators(.hidden)
                .padding(.trailing, -16)
            }
        }
    }
}

struct AlbumCard: View {
    let album: Album
    var width: CGFloat?

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Cover(path: album.artworkPath, size: width, radius: 10)
            VStack(alignment: .leading, spacing: 0) {
                Text(album.title).font(Theme.small).lineLimit(2)
                Text(album.artist ?? "Various artists").font(Theme.small).foregroundStyle(Theme.quiet).lineLimit(2)
            }
        }
        .frame(width: width)
        .accessibilityElement(children: .combine)
    }
}
