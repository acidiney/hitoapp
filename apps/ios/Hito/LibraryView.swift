import HitoKit
import SwiftUI

struct LibraryView: View {
    enum Section: String, CaseIterable {
        case tracks = "Tracks"
        case albums = "Albums"
        case artists = "Artists"
        case playlists = "Playlists"
    }

    @Environment(AppModel.self) private var model
    @State private var section = Section.tracks
    @State private var tracks = Feed<Track>()
    @State private var albums = Feed<Album>()
    @State private var artists = Feed<Artist>()
    @State private var playlists: [Playlist]?
    @State private var playlistsFailed = false
    @State private var sort = TrackSort.recent
    @State private var filtering = false
    @State private var importing = false
    @State private var selected: Set<String>?
    @State private var notice: String?
    @State private var opened: Playlist?

    var body: some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 12) {
                HStack {
                    Text("Your library").font(Theme.title).accessibilityAddTraits(.isHeader)
                    Spacer(minLength: 8)
                    if section == .tracks {
                        Button { filtering = true } label: {
                            HStack(spacing: 8) {
                                Icon("filters", size: 18)
                                Text(sort == .recent ? "Filters" : "Filters · 1").font(Theme.smallMedium)
                            }
                            .foregroundStyle(Theme.text)
                            .padding(.horizontal, 14)
                            .frame(minHeight: 44)
                            .overlay(Capsule().strokeBorder(sort == .recent ? Theme.line : Theme.accent, lineWidth: sort == .recent ? 1 : 1.5))
                        }
                        .buttonStyle(.plain)
                        .accessibilityLabel(sort == .recent ? "Filter and sort" : "Filter and sort, sorted by \(sort.label)")
                    }
                }
                SectionTabs(selection: $section)
                switch section {
                case .tracks:
                    TrackRows(feed: tracks, context: "Your library", empty: "No tracks yet. An administrator adds music folders on the web.", selection: $selected, selectable: true)
                        .padding(.leading, -6)
                        .padding(.trailing, -8)
                case .albums:
                    AlbumGrid(feed: albums)
                case .artists:
                    ArtistRows(feed: artists)
                case .playlists:
                    Button { importing = true } label: {
                        HStack(spacing: 12) {
                            ZStack { Theme.tile; Icon("link", size: 20).foregroundStyle(Theme.focus) }
                                .frame(width: 44, height: 44)
                                .clipShape(RoundedRectangle(cornerRadius: 6))
                            Text("Import a playlist from a link").font(Theme.bodyMedium)
                            Spacer(minLength: 0)
                        }
                        .frame(minHeight: 60)
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    PlaylistRows(playlists: playlists, failed: playlistsFailed) { Task { await loadPlaylists() } }
                }
            }
            .padding(.horizontal, 16)
            .padding(.bottom, 16)
        }
        .scrollIndicators(.hidden)
        .refreshable { reload() }
        .fullScreenCover(isPresented: $filtering) {
            FilterSheet(sort: $sort, total: tracks.total)
                .presentationBackground(.clear)
        }
        .safeAreaInset(edge: .bottom, spacing: 0) {
            if let chosen = selected {
                SelectionBar(
                    count: chosen.count,
                    loaded: tracks.items.count,
                    playlists: (playlists ?? []).filter { $0.kind == .local },
                    selectLoaded: { selected = Set(tracks.items.map(\.id)) },
                    queue: { queueSelected(chosen) },
                    add: { playlist in Task { await add(chosen, to: playlist) } },
                    close: { selected = nil }
                )
            } else if let notice {
                Text(notice).font(Theme.small).foregroundStyle(Theme.text)
                    .padding(.horizontal, 16).frame(maxWidth: .infinity, minHeight: 44, alignment: .leading)
                    .background(Theme.raised)
                    .task { try? await Task.sleep(for: .seconds(3)); self.notice = nil }
            }
        }
        .onChange(of: selected == nil) { if selected != nil, playlists == nil { Task { await loadPlaylists() } } }
        .onChange(of: section) { selected = nil }
        .fullScreenCover(isPresented: $importing) {
            ImportSheet { id in Task { await loadPlaylists(); opened = playlists?.first { $0.id == id } } }
                .presentationBackground(.clear)
        }
        .navigationDestination(item: $opened) { PlaylistView(playlist: $0).pushed() }
        .onChange(of: sort) { reloadTracks() }
        .task {
            if tracks.total == nil && albums.total == nil { reload() }
        }
    }

    private func reload() {
        guard let api = model.api else { return }
        let library = Library(api: api)
        reloadTracks()
        albums.reset { try await library.albums(offset: $0) }
        artists.reset { try await library.artists(offset: $0) }
        Task { await loadPlaylists() }
    }

    private func reloadTracks() {
        guard let api = model.api else { return }
        let library = Library(api: api)
        let sort = sort
        tracks.reset { try await library.tracks(offset: $0, sort: sort) }
    }

    private func queueSelected(_ chosen: Set<String>) {
        let picked = tracks.items.filter { chosen.contains($0.id) && $0.isPlayable }
        picked.forEach { model.player?.playNext($0) }
        notice = picked.count == chosen.count ? "Added \(picked.count) to the queue" : "Added \(picked.count) to the queue. Tracks without a source were left out."
        selected = nil
    }

    private func add(_ chosen: Set<String>, to playlist: Playlist) async {
        guard let api = model.api else { return }
        let ordered = tracks.items.map(\.id).filter { chosen.contains($0) }
        do {
            try await Playlists(api: api).insert(ordered, into: playlist.id, device: Player.device, from: Int(Date().timeIntervalSince1970 * 1000))
            notice = "Added \(ordered.count) to \(playlist.name)"
            selected = nil
        } catch {
            notice = "\(playlist.name) didn't change. Try again."
            model.handle(error)
        }
    }

    private func loadPlaylists() async {
        guard let api = model.api else { return }
        playlistsFailed = false
        do {
            playlists = try await Playlists(api: api).list()
        } catch {
            playlistsFailed = true
            model.handle(error)
        }
    }
}

struct ArtistRows: View {
    let feed: Feed<Artist>

    var body: some View {
        if feed.isEmpty {
            Text("No artists yet.").font(Theme.small).foregroundStyle(Theme.soft).padding(.vertical, 12)
        }
        ForEach(feed.items) { artist in
            NavigationLink(value: TrackList.artist(artist)) {
                HStack(spacing: 12) {
                    ZStack {
                        Theme.tile
                        Icon("person", size: 20).foregroundStyle(Theme.coverMark)
                    }
                    .frame(width: 44, height: 44)
                    .clipShape(Circle())
                    VStack(alignment: .leading, spacing: 0) {
                        Text(artist.name).lineLimit(1)
                        Text(counts(artist)).font(Theme.small).foregroundStyle(Theme.quiet).lineLimit(1)
                    }
                    Spacer(minLength: 8)
                    Icon("chevron-right", size: 16).foregroundStyle(Theme.quiet)
                }
                .frame(minHeight: 60)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
        }
        FeedFooter(feed: feed)
    }

    private func counts(_ artist: Artist) -> String {
        let tracks = "\(artist.trackCount) \(artist.trackCount == 1 ? "track" : "tracks")"
        let albums = "\(artist.albumCount) \(artist.albumCount == 1 ? "album" : "albums")"
        return "\(tracks) · \(albums)"
    }
}

struct PlaylistRows: View {
    let playlists: [Playlist]?
    let failed: Bool
    let retry: () -> Void

    var body: some View {
        if let playlists {
            if playlists.isEmpty {
                Text("No playlists yet. Import one from a link, or make one on the web.")
                    .font(Theme.small)
                    .foregroundStyle(Theme.soft)
                    .padding(.vertical, 12)
            }
            ForEach(playlists) { playlist in
                NavigationLink(value: playlist) {
                    HStack(spacing: 12) {
                        Cover(path: playlist.artworkPath, size: 44, mark: "playlist")
                        VStack(alignment: .leading, spacing: 0) {
                            Text(playlist.name).lineLimit(1)
                            Text(playlist.kind == .followed ? "Followed playlist" : "Playlist")
                                .font(Theme.small)
                                .foregroundStyle(Theme.quiet)
                        }
                        Spacer(minLength: 8)
                        Icon("chevron-right", size: 16).foregroundStyle(Theme.quiet)
                    }
                    .frame(minHeight: 60)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
            }
        } else if failed {
            HStack(spacing: 8) {
                Text("Playlists didn't load.").foregroundStyle(Theme.soft)
                TextAction(title: "Try again", action: retry)
            }
            .font(Theme.small)
        } else {
            ProgressView().tint(Theme.soft).frame(maxWidth: .infinity, minHeight: 60)
        }
    }
}

struct SectionTabs: View {
    @Binding var selection: LibraryView.Section

    var body: some View {
        ScrollView(.horizontal) {
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
        }
        .padding(.horizontal, 16)
        }
        .scrollIndicators(.hidden)
        .overlay(alignment: .bottom) { Theme.barLine.frame(height: 1) }
        .padding(.horizontal, -16)
    }
}

struct TrackRows: View {
    @Environment(AppModel.self) private var model
    let feed: Feed<Track>
    let context: String
    let empty: String
    var selection: Binding<Set<String>?> = .constant(nil)
    var selectable = false

    var body: some View {
        if feed.isEmpty {
            Text(empty).font(Theme.small).foregroundStyle(Theme.soft).padding(.vertical, 12)
        }
        ForEach(Array(feed.items.enumerated()), id: \.element.id) { index, track in
            if let chosen = selection.wrappedValue {
                let picked = chosen.contains(track.id)
                Button {
                    selection.wrappedValue = picked ? chosen.subtracting([track.id]) : chosen.union([track.id])
                } label: {
                    HStack(spacing: 10) {
                        ZStack {
                            RoundedRectangle(cornerRadius: 5).fill(picked ? Theme.accent : .clear)
                            RoundedRectangle(cornerRadius: 5).strokeBorder(picked ? Theme.accent : Theme.quiet, lineWidth: 1.5)
                            if picked { Icon("check", size: 14).foregroundStyle(Theme.onAccent) }
                        }
                        .frame(width: 22, height: 22)
                        .padding(.leading, 10)
                        LibraryRow(track: track, isCurrent: false)
                    }
                    .background(RoundedRectangle(cornerRadius: 12).fill(picked ? Theme.accent.opacity(0.16) : .clear))
                }
                .buttonStyle(.plain)
                .accessibilityAddTraits(picked ? [.isButton, .isSelected] : .isButton)
            } else {
                Button {
                    model.player?.play(feed.items, at: index, from: context)
                } label: {
                    LibraryRow(track: track, isCurrent: model.player?.current?.id == track.id)
                }
                .buttonStyle(.plain)
                .disabled(!track.isPlayable)
                .contextMenu {
                    TrackMenu(track: track)
                    if selection.wrappedValue == nil, selectable {
                        Button("Select") { selection.wrappedValue = [track.id] }
                    }
                }
            }
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
                    TrackRows(feed: feed, context: list.title, empty: list.empty)
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
            let list = list
            feed.reset { try await list.page(library, $0) }
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
                        Text(["Album", album.year.map(String.init)].compactMap { $0 }.joined(separator: " · "))
                            .font(Theme.small)
                            .foregroundStyle(Theme.soft)
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
        let length = album.durationMs.flatMap { $0 >= 60_000 ? Duration.milliseconds($0).long : nil }
        return [album.artist ?? "Various artists", count, length].compactMap { $0 }.joined(separator: " · ")
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

struct FilterSheet: View {
    @Environment(\.dismiss) private var dismiss
    @Binding var sort: TrackSort
    let total: Int?

    var body: some View {
        VStack(spacing: 0) {
            Color.black.opacity(0.55)
                .onTapGesture { dismiss() }
                .accessibilityHidden(true)
            sheet
        }
        .ignoresSafeArea(edges: .top)
    }

    private var sheet: some View {
        VStack(alignment: .leading, spacing: 20) {
            Capsule().fill(Theme.line).frame(width: 40, height: 4).frame(maxWidth: .infinity)
            HStack {
                Text("Filter and sort").font(Theme.playing).accessibilityAddTraits(.isHeader)
                Spacer()
                IconButton(icon: "close", label: "Close", glyph: 18) { dismiss() }
            }
            VStack(alignment: .leading, spacing: 8) {
                Text("Sort").font(Theme.small).foregroundStyle(Theme.quiet)
                FlowChips(items: TrackSort.allCases, label: \.label, selected: sort) { sort = $0 }
            }
            HStack(spacing: 8) {
                Button { dismiss() } label: {
                    Text(total.map { "Show \($0) \($0 == 1 ? "track" : "tracks")" } ?? "Show tracks")
                        .font(Theme.bodyStrong)
                        .foregroundStyle(Theme.onAccent)
                        .frame(maxWidth: .infinity, minHeight: 48)
                        .background(Capsule().fill(Theme.accent))
                }
                .buttonStyle(.plain)
                TextAction(title: "Reset") { sort = .recent }
                    .padding(.horizontal, 8)
            }
        }
        .padding(.horizontal, 16)
        .padding(.top, 8)
        .padding(.bottom, 12)
        .foregroundStyle(Theme.text)
        .background(UnevenRoundedRectangle(topLeadingRadius: 16, topTrailingRadius: 16).fill(Theme.field).ignoresSafeArea(edges: .bottom))
        .accessibilityAddTraits(.isModal)
    }
}

struct FlowChips<Item: Hashable>: View {
    let items: [Item]
    let label: KeyPath<Item, String>
    let selected: Item
    let pick: (Item) -> Void

    var body: some View {
        ViewThatFits(in: .horizontal) {
            HStack(spacing: 8) { chips }
            VStack(alignment: .leading, spacing: 8) { chips }
        }
        .accessibilityElement(children: .contain)
    }

    private var chips: some View {
        ForEach(items, id: \.self) { item in
            let on = item == selected
            Button { pick(item) } label: {
                Text(item[keyPath: label])
                    .font(Theme.smallMedium)
                    .foregroundStyle(on ? Theme.ground : Theme.text)
                    .padding(.horizontal, 16)
                    .frame(minHeight: 44)
                    .background(Capsule().fill(on ? Theme.text : .clear))
                    .overlay(Capsule().strokeBorder(on ? Theme.text : Theme.line))
            }
            .buttonStyle(.plain)
            .accessibilityAddTraits(on ? [.isSelected] : [])
        }
    }
}

struct SelectionBar: View {
    let count: Int
    let loaded: Int
    let playlists: [Playlist]
    let selectLoaded: () -> Void
    let queue: () -> Void
    let add: (Playlist) -> Void
    let close: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 12) {
                Text("\(count) selected").font(Theme.bodyStrong)
                Spacer(minLength: 0)
                if count < loaded {
                    TextAction(title: "Select the \(loaded) loaded", action: selectLoaded)
                }
                IconButton(icon: "close", label: "Stop selecting", glyph: 18, action: close)
            }
            HStack(spacing: 10) {
                Menu {
                    if playlists.isEmpty {
                        Text("No playlists of yours yet")
                    }
                    ForEach(playlists) { playlist in
                        Button(playlist.name) { add(playlist) }
                    }
                } label: {
                    Text("Add to playlist")
                        .font(Theme.bodyStrong)
                        .foregroundStyle(Theme.onAccent)
                        .frame(maxWidth: .infinity, minHeight: 48)
                        .background(Capsule().fill(Theme.accent))
                }
                .disabled(count == 0)
                Button(action: queue) {
                    Text("Add to queue")
                        .font(Theme.bodyMedium)
                        .foregroundStyle(Theme.text)
                        .frame(maxWidth: .infinity, minHeight: 48)
                        .overlay(Capsule().strokeBorder(Theme.line))
                }
                .buttonStyle(.plain)
                .disabled(count == 0)
            }
        }
        .padding(16)
        .background(RoundedRectangle(cornerRadius: 18).fill(Color(hex: 0x1f2340)))
        .padding(.horizontal, 12)
        .padding(.bottom, 8)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Selection actions")
    }
}
