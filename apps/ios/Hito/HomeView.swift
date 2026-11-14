import HitoKit
import SwiftUI

struct HomeView: View {
    @Environment(AppModel.self) private var model
    let user: User
    let openSearch: () -> Void
    var openDiscover: () -> Void = {}
    var openPlugins: () -> Void = {}
    @State private var albums = Feed<Album>()
    @State private var importing = false
    @State private var opened: Playlist?
    @State private var summary: HomeSummary?
    @State private var playlists: [Playlist] = []
    @State private var unready: Playlists.Unready?

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
                        .font(Theme.title).tracking(Theme.titleTracking)
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
                if let summary, summary.isEmpty(playlists: playlists) {
                    HomeEmpty(admin: user.admin, importPlaylist: { importing = true }, connectService: openPlugins)
                } else {
                    if let summary {
                        JumpBackIn(tiles: summary.tiles(playlists: playlists))
                        ForEach(summary.unheard.prefix(1)) { UnheardCard(album: $0) }
                    } else if !albums.isEmpty {
                        JumpBackIn(tiles: QuickTile.mixed(albums: albums.items, playlists: playlists))
                    }
                    if let unready {
                        UnreadyBanner(unready: unready)
                    }
                    RecentlyAdded(feed: albums)
                    if let summary {
                        RecentlyPlayed(albums: summary.recentAlbums)
                    }
                    Button(action: openDiscover) {
                        (Text("Looking for something new? ").foregroundStyle(Theme.soft) + Text("Discover music from your services").foregroundStyle(Theme.focus))
                            .font(Theme.small)
                            .multilineTextAlignment(.leading)
                            .frame(minHeight: 44, alignment: .leading)
                    }
                    .buttonStyle(.plain)
                }
            }
            .padding(.horizontal, 16)
            .padding(.bottom, 16)
        }
        .scrollIndicators(.hidden)
        .fullScreenCover(isPresented: $importing) {
            ImportSheet { id in
                Task {
                    guard let api = model.api else { return }
                    opened = (try? await Playlists(api: api).list())?.first { $0.id == id }
                }
            }
            .presentationBackground(.clear)
        }
        .navigationDestination(item: $opened) { PlaylistView(playlist: $0).pushed(tint: Theme.header) }
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
        let lists = Playlists(api: api)
        playlists = (try? await lists.list()) ?? []
        unready = await lists.leastReady(playlists, checking: 4)
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
                            ZStack { Theme.tile; Icon("playlist", size: 24.6).foregroundStyle(Theme.coverMark) }
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

struct UnreadyBanner: View {
    let unready: Playlists.Unready

    var body: some View {
        NavigationLink(value: unready.playlist) {
            HStack(spacing: 10) {
                Circle().strokeBorder(Theme.warning, lineWidth: 1.5).frame(width: 10, height: 10)
                Text("\(unready.unready) \(unready.unready == 1 ? "track" : "tracks") in \(unready.playlist.name) \(unready.unready == 1 ? "isn't" : "aren't") ready")
                    .font(Theme.small)
                    .foregroundStyle(Theme.text)
                    .lineLimit(1)
                    .frame(maxWidth: .infinity, alignment: .leading)
                Text("Find sources").font(Theme.smallBold).foregroundStyle(Theme.focus)
            }
            .padding(.horizontal, 14)
            .frame(minHeight: 48)
            .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(Theme.barLine))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityHint("Opens \(unready.playlist.name)")
    }
}

struct UnheardCard: View {
    @Environment(AppModel.self) private var model
    let album: UnheardAlbum

    var body: some View {
        HStack(spacing: 14) {
            NavigationLink(value: album.album) {
                HStack(spacing: 14) {
                    Cover(path: album.album.artworkPath, size: 84, radius: 10)
                    VStack(alignment: .leading, spacing: 2) {
                        Text("Unheard, near what you like").font(Theme.label).foregroundStyle(Theme.focus)
                        Text(album.headline).font(Theme.heading).fixedSize(horizontal: false, vertical: true)
                        Text(album.reason).font(Theme.small).foregroundStyle(Theme.quiet).fixedSize(horizontal: false, vertical: true)
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            Button {
                if !album.tracks.isEmpty { model.player?.play(album.tracks, at: 0, from: album.title) }
            } label: {
                Icon("play", size: 18)
                    .foregroundStyle(Theme.onAccent)
                    .frame(width: 48, height: 48)
                    .background(Circle().fill(Theme.accent))
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Play \(album.headline)")
        }
        .padding(12)
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

struct HomeEmpty: View {
    let admin: Bool
    let importPlaylist: () -> Void
    let connectService: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("Your library is still empty").font(Theme.pageTitle).accessibilityAddTraits(.isHeader)
            Text(admin ? "Start with one of these. You can do all three." : "Ask your administrator to give you access to a music library, or start with a playlist or a service.")
                .font(Theme.small)
                .foregroundStyle(Theme.soft)
            if admin {
                NavigationLink(value: SettingsPage.music) {
                    Text("Choose a music folder")
                        .font(Theme.bodyStrong)
                        .foregroundStyle(Theme.onAccent)
                        .frame(maxWidth: .infinity, minHeight: 48)
                        .background(Capsule().fill(Theme.accent))
                }
                .buttonStyle(.plain)
            }
            Button(action: importPlaylist) {
                Text("Import a playlist")
                    .font(Theme.bodyMedium)
                    .frame(maxWidth: .infinity, minHeight: 48)
                    .overlay(Capsule().strokeBorder(Theme.accent))
            }
            .buttonStyle(.plain)
            Button(action: connectService) {
                Text("Connect a service")
                    .font(Theme.bodyMedium)
                    .frame(maxWidth: .infinity, minHeight: 48)
                    .overlay(Capsule().strokeBorder(Theme.line))
            }
            .buttonStyle(.plain)
        }
        .foregroundStyle(Theme.text)
        .padding(18)
        .background(RoundedRectangle(cornerRadius: 18).fill(Theme.field))
    }
}

struct RecentlyPlayed: View {
    let albums: [Album]

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Recently played").font(Theme.section).accessibilityAddTraits(.isHeader)
            if albums.isEmpty {
                Text("Play something and it will show up here.").font(Theme.small).foregroundStyle(Theme.soft)
            } else {
                ScrollView(.horizontal) {
                    LazyHStack(alignment: .top, spacing: 12) {
                        ForEach(albums) { album in
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
                Text(album.title).font(Theme.small).lineLimit(1)
                if let credit = album.credit {
                    Text(credit).font(Theme.small).foregroundStyle(Theme.quiet).lineLimit(1)
                }
            }
        }
        .frame(width: width)
        .accessibilityElement(children: .combine)
    }
}
