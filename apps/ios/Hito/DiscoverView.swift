import HitoKit
import SwiftUI

struct DiscoverView: View {
    @Environment(AppModel.self) private var model
    let openSearch: () -> Void
    @State private var artists: [RadioArtist]?
    @State private var heroLabel = ""
    @State private var hero: ArtistRadio?
    @State private var shelf: FeaturedShelf?
    @State private var starting: String?
    @State private var problem: String?
    @State private var opened: Playlist?

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 28) {
                Text("Discover").font(Theme.title).accessibilityAddTraits(.isHeader)
                if let problem {
                    Text(problem).font(Theme.small).foregroundStyle(Theme.warning)
                }
                if let artists {
                    if let first = artists.first {
                        heroCard(first)
                        artistSection(artists)
                    } else {
                        Text("No artist radios yet. Radios start from artists Hito has matched on MusicBrainz, and none in your library are matched yet.")
                            .font(Theme.small).foregroundStyle(Theme.soft)
                    }
                } else {
                    ProgressView().tint(Theme.soft).frame(maxWidth: .infinity, minHeight: 120)
                }
                outside
            }
            .padding(.horizontal, 16)
            .padding(.top, 12)
            .padding(.bottom, 24)
        }
        .scrollIndicators(.hidden)
        .page()
        .refreshable { await load() }
        .task { if artists == nil { await load() } }
        .navigationDestination(item: $opened) { PlaylistView(playlist: $0).pushed() }
    }

    private func heroCard(_ artist: RadioArtist) -> some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack(spacing: 14) {
                ZStack { Circle().fill(Theme.tile); Icon("person", size: 30).foregroundStyle(Theme.quiet) }
                    .frame(width: 72, height: 72)
                    .accessibilityHidden(true)
                VStack(alignment: .leading, spacing: 2) {
                    Text(heroLabel).font(Theme.smallStrong).foregroundStyle(Theme.soft)
                    Text("\(artist.name) radio").font(Theme.pageTitle).lineLimit(2)
                }
            }
            Text(hero?.reach ?? "\(artist.tracks) \(artist.tracks == 1 ? "track" : "tracks") in your library")
                .font(Theme.small).foregroundStyle(Theme.soft)
            if let hero, !hero.tracks.isEmpty {
                VStack(alignment: .leading, spacing: 0) {
                    Text("First up").font(Theme.smallStrong).foregroundStyle(Theme.quiet)
                    ForEach(hero.tracks.prefix(3)) { track in
                        LibraryRow(track: track, isCurrent: model.player?.current?.id == track.id)
                    }
                }
            }
            Button { Task { await start(artist) } } label: {
                HStack(spacing: 8) {
                    if starting == artist.id { ProgressView().tint(Theme.onAccent) } else { Icon("play", size: 16) }
                    Text("Start radio").font(Theme.bodyStrong)
                }
                .foregroundStyle(Theme.onAccent)
                .padding(.horizontal, 22)
                .frame(minHeight: 48)
                .background(Capsule().fill(Theme.accent))
            }
            .buttonStyle(.plain)
            .disabled(starting != nil)
        }
        .padding(16)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: 18).fill(Theme.field))
    }

    private func artistSection(_ artists: [RadioArtist]) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("Start a radio from an artist").font(Theme.section).accessibilityAddTraits(.isHeader)
            ForEach(artists) { artist in
                Button { Task { await start(artist) } } label: {
                    HStack(spacing: 12) {
                        ZStack { Circle().fill(Theme.tile); Icon("person", size: 18).foregroundStyle(Theme.quiet) }
                            .frame(width: 44, height: 44)
                        VStack(alignment: .leading, spacing: 0) {
                            Text(artist.name).font(Theme.bodyMedium).lineLimit(1)
                            Text("\(artist.tracks) \(artist.tracks == 1 ? "track" : "tracks") in your library")
                                .font(Theme.small).foregroundStyle(Theme.quiet)
                        }
                        Spacer(minLength: 8)
                        if starting == artist.id {
                            ProgressView().tint(Theme.soft)
                        } else {
                            Icon("play", size: 14).foregroundStyle(Theme.accent)
                        }
                    }
                    .padding(.horizontal, 10)
                    .frame(minHeight: 60)
                    .background(Capsule().fill(Theme.field))
                    .contentShape(Capsule())
                }
                .buttonStyle(.plain)
                .disabled(starting != nil)
                .accessibilityLabel("Start \(artist.name) radio")
            }
        }
    }

    private var outside: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Outside your library").font(Theme.section).accessibilityAddTraits(.isHeader)
            Text("Search a plugin directly. What you find stays outside your library until you add it.")
                .font(Theme.small).foregroundStyle(Theme.soft)
            Button(action: openSearch) {
                HStack(spacing: 10) {
                    Icon("search", size: 18).foregroundStyle(Theme.quiet)
                    Text("Search a plugin").foregroundStyle(Theme.quiet)
                    Spacer()
                }
                .padding(.horizontal, 16)
                .frame(minHeight: 48)
                .overlay(Capsule().strokeBorder(Theme.line))
                .contentShape(Capsule())
            }
            .buttonStyle(.plain)
            if case let .available(items) = shelf, !items.isEmpty {
                Text("Featured · chosen by the plugin, not by Hito").font(Theme.smallStrong).foregroundStyle(Theme.soft)
                    .padding(.top, 6)
                ForEach(items) { item in
                    HStack(spacing: 12) {
                        ZStack { Theme.tile; Icon("playlist", size: 20).foregroundStyle(Theme.quiet) }
                            .frame(width: 48, height: 48)
                            .clipShape(RoundedRectangle(cornerRadius: 6))
                        VStack(alignment: .leading, spacing: 0) {
                            Text(item.title).font(Theme.bodyMedium).lineLimit(2)
                            Text(item.detail).font(Theme.small).foregroundStyle(Theme.quiet).lineLimit(1)
                        }
                        Spacer(minLength: 4)
                        RoundAction(icon: "play", label: "Play \(item.title)", busy: starting == item.id) { Task { await play(item) } }
                        RoundAction(icon: "plus", label: "Open \(item.title)", busy: false) { Task { await open(item) } }
                    }
                    .padding(10)
                    .background(RoundedRectangle(cornerRadius: 14).fill(Theme.field))
                }
            }
        }
    }

    private func load() async {
        guard let api = model.api else { return }
        let discovery = Discovery(api: api)
        problem = nil
        do {
            let found = try await discovery.artists()
            artists = found.items
            heroLabel = found.heroLabel
            if let first = found.items.first {
                hero = try? await discovery.radio(first.id)
            }
        } catch {
            artists = []
            problem = "Discover didn't load. Pull to try again."
            model.handle(error)
        }
        shelf = try? await discovery.featured()
    }

    private func start(_ artist: RadioArtist) async {
        guard let api = model.api else { return }
        starting = artist.id
        problem = nil
        defer { starting = nil }
        do {
            let radio: ArtistRadio
            if artist.id == artists?.first?.id, let hero {
                radio = hero
            } else {
                radio = try await Discovery(api: api).radio(artist.id)
            }
            let ready = radio.tracks.filter(\.isPlayable)
            guard !ready.isEmpty else {
                problem = "\(artist.name) radio has nothing ready to play yet."
                return
            }
            model.player?.play(ready, at: 0, from: "\(artist.name) radio")
        } catch {
            problem = "\(artist.name) radio didn't start. Try again."
            model.handle(error)
        }
    }

    private func open(_ item: FeaturedItem) async {
        guard let api = model.api else { return }
        problem = nil
        do {
            let id = try await Discovery(api: api).open(item)
            opened = Playlist(id: id, name: item.title, kind: .followed)
        } catch {
            problem = "\(item.title) didn't open. Try again."
            model.handle(error)
        }
    }

    private func play(_ item: FeaturedItem) async {
        guard let api = model.api else { return }
        starting = item.id
        problem = nil
        defer { starting = nil }
        do {
            let id = try await Discovery(api: api).open(item)
            let detail = try await Playlists(api: api).detail(id)
            let ready = detail.tracks.filter(\.isPlayable)
            guard !ready.isEmpty else {
                opened = detail.playlist
                problem = "Nothing in \(item.title) is ready to play yet."
                return
            }
            model.player?.play(ready, at: 0, from: item.title)
        } catch {
            problem = "\(item.title) didn't play. Try again."
            model.handle(error)
        }
    }
}
