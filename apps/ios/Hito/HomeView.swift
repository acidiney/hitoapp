import HitoKit
import SwiftUI

struct HomeView: View {
    @Environment(AppModel.self) private var model
    let user: User
    let openSearch: () -> Void
    @State private var albums = Feed<Album>()

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
                        Text(String(name.prefix(1)).uppercased())
                            .font(Theme.bodyBold)
                            .frame(width: 44, height: 44)
                            .background(Circle().fill(Theme.avatar))
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("Settings for \(name)")
                }
                Button(action: openSearch) {
                    HStack(spacing: 10) {
                        Icon("search")
                        Text("Search your library")
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
                if !albums.isEmpty {
                    JumpBackIn(albums: Array(albums.items.prefix(5)))
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
        }
        .task {
            guard albums.total == nil else { return }
            reload()
            await albums.more(model)
        }
    }

    private func reload() {
        guard let api = model.api else { return }
        let library = Library(api: api)
        albums.reset { try await library.albums(offset: $0) }
    }
}

struct JumpBackIn: View {
    let albums: [Album]

    var body: some View {
        LazyVGrid(columns: [GridItem(.flexible(), spacing: 8), GridItem(.flexible(), spacing: 8)], spacing: 8) {
            NavigationLink(value: TrackList.liked) {
                Tile(title: "Liked tracks") {
                    ZStack {
                        Theme.accent
                        Icon("heart-filled", size: 24.6).foregroundStyle(Theme.onAccent)
                    }
                    .frame(width: 56, height: 56)
                }
            }
            ForEach(albums) { album in
                NavigationLink(value: album) {
                    Tile(title: album.title) { Cover(path: album.artworkPath, size: 56, radius: 0) }
                }
            }
        }
        .buttonStyle(.plain)
        .accessibilityLabel("Jump back in")
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
                Text(album.title).font(Theme.small).lineLimit(1)
                Text(album.artist ?? "Various artists").font(Theme.small).foregroundStyle(Theme.quiet).lineLimit(1)
            }
        }
        .frame(width: width)
        .accessibilityElement(children: .combine)
    }
}
