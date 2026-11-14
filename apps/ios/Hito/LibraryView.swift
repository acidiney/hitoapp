import HitoKit
import SwiftUI

struct LibraryView: View {
    enum Section: String, CaseIterable {
        case tracks = "Tracks"
        case albums = "Albums"
        case artists = "Artists"
        case playlists = "Playlists"
        case podcasts = "Podcasts"
        case audiobooks = "Audiobooks"
        case history = "History"
    }

    @Environment(AppModel.self) private var model
    @State private var section = Section.tracks
    @State private var tracks = Feed<Track>()
    @State private var albums = Feed<Album>()
    @State private var artists = Feed<Artist>()
    @State private var playlists: [Playlist]?
    @State private var shown = TrackShown.all
    @State private var filters = false
    @State private var playlistsFailed = false
    @State private var sort = TrackSort.recent
    @State private var filtering = false
    @State private var importing = false
    @State private var selected: Set<String>?
    @State private var notice: String?
    @State private var opened: Playlist?
    @State private var naming = false
    @State private var newName = ""
    @State private var finding: String?

    var active: Int { (sort == .recent ? 0 : 1) + (shown == .all ? 0 : 1) }

    var body: some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 12) {
                HStack {
                    Text("Your library").font(Theme.title).tracking(Theme.titleTracking).accessibilityAddTraits(.isHeader)
                    Spacer(minLength: 8)
                    if section == .tracks {
                        Button { filtering = true } label: {
                            HStack(spacing: 8) {
                                Icon("filters", size: 16)
                                Text(active == 0 ? "Filters" : "Filters · \(active)").font(Theme.smallMedium)
                            }
                            .foregroundStyle(Theme.text)
                            .padding(.horizontal, 14)
                            .frame(minHeight: 44)
                            .background(Capsule().fill(Theme.field))
                            .overlay(Capsule().strokeBorder(active == 0 ? Theme.line : Theme.accent))
                        }
                        .buttonStyle(.plain)
                        .accessibilityLabel(active == 0 ? "Filter and sort" : "Filter and sort, \(active) active")
                    }
                }
                SectionTabs(selection: $section)
                switch section {
                case .tracks:
                    if active > 0 {
                        Wrap(spacing: 8) {
                            if shown != .all {
                                ActiveChip(title: shown.label) { shown = .all }
                            }
                            if sort != .recent {
                                ActiveChip(title: "Sorted by \(sort.label.lowercased())") { sort = .recent }
                            }
                            TextAction(title: "Reset", inset: 12) {
                                shown = .all
                                sort = .recent
                            }
                        }
                    }
                    TrackRows(feed: tracks, context: "Your library", empty: "No tracks yet. An administrator adds music folders on the web.", selection: $selected, selectable: true)
                        .padding(.leading, -6)
                        .padding(.trailing, -8)
                case .albums:
                    AlbumGrid(feed: albums)
                case .artists:
                    ArtistRows(feed: artists)
                case .playlists:
                    Button { newName = ""; naming = true } label: {
                        HStack(spacing: 12) {
                            ZStack { Theme.tile; Icon("plus", size: 20).foregroundStyle(Theme.focus) }
                                .frame(width: 44, height: 44)
                                .clipShape(RoundedRectangle(cornerRadius: 6))
                            Text("New playlist").font(Theme.bodyMedium)
                            Spacer(minLength: 0)
                        }
                        .frame(minHeight: 60)
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
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
                case .podcasts:
                    PodcastsSection()
                case .audiobooks:
                    AudiobooksSection()
                case .history:
                    HistorySection()
                }
            }
            .padding(.horizontal, 16)
            .padding(.bottom, 16)
        }
        .scrollIndicators(.hidden)
        .refreshable { reload() }
        .fullScreenCover(isPresented: $filtering) {
            FilterSheet(sort: $sort, shown: $shown, filters: filters, total: tracks.unsupported == nil ? tracks.total : nil)
                .presentationBackground(.clear)
        }
        .safeAreaInset(edge: .bottom, spacing: 0) {
            if let chosen = selected {
                SelectionBar(
                    count: chosen.count,
                    loaded: tracks.items.count,
                    playlists: (playlists ?? []).filter { $0.kind == .local },
                    finding: finding,
                    selectLoaded: { selected = Set(tracks.items.map(\.id)) },
                    find: { findSelected(chosen) },
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
        .onChange(of: section) {
            selected = nil
            if section == .playlists { Task { await loadPlaylists() } }
        }
        .fullScreenCover(isPresented: $importing) {
            ImportSheet { id in Task { await loadPlaylists(); opened = playlists?.first { $0.id == id } } }
                .presentationBackground(.clear)
        }
        .navigationDestination(item: $opened) { PlaylistView(playlist: $0).pushed(tint: Theme.header) }
        .alert("New playlist", isPresented: $naming) {
            TextField("Give it a name", text: $newName)
            Button("Cancel", role: .cancel) {}
            Button("Create") { Task { await create() } }
        }
        .onChange(of: sort) { reloadTracks() }
        .onChange(of: shown) { reloadTracks() }
        .task {
            if tracks.total == nil && albums.total == nil { reload() } else if section == .playlists { await loadPlaylists() }
            if let api = model.api { filters = (try? await Library(api: api).filtersByAvailability()) ?? false }
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
        let availability = shown.availability
        tracks.reset { try await library.tracks(offset: $0, sort: sort, availability: availability) }
    }

    private func findSelected(_ chosen: Set<String>) {
        guard let api = model.api, finding == nil else { return }
        let ids = tracks.items.filter { chosen.contains($0.id) && $0.canFind }.map(\.id)
        guard !ids.isEmpty else {
            notice = "The selected tracks already have a source."
            return
        }
        var done = 0
        finding = "Finding 1 of \(ids.count)…"
        Task {
            do {
                let found = try await Playback(api: api).find(ids) { id, result in
                    done += 1
                    finding = done < ids.count ? "Finding \(done + 1) of \(ids.count)…" : nil
                    if result == .found, let track = tracks.items.first(where: { $0.id == id }) {
                        tracks.replace(id, with: track.readied)
                    }
                }
                notice = found == 0 ? "No sources found for the selected tracks." : "Found sources for \(found) of \(ids.count)."
                selected = nil
            } catch {
                notice = "The search stopped. Try again."
                model.handle(error)
            }
            finding = nil
        }
    }

    private func create() async {
        let name = newName.trimmingCharacters(in: .whitespaces)
        guard let api = model.api, !name.isEmpty else { return }
        do {
            let id = try await Playlists(api: api).create(named: name, device: Player.device)
            await loadPlaylists()
            opened = playlists?.first { $0.id == id } ?? Playlist(id: id, name: name, kind: .local)
        } catch {
            notice = "The playlist wasn't created. Try again."
        }
    }

    private func queueSelected(_ chosen: Set<String>) {
        let picked = tracks.items.filter { chosen.contains($0.id) }
        picked.forEach { model.player?.playNext($0) }
        notice = "Added \(picked.count) to the queue"
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
            NavigationLink(value: artist) {
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
                Text("No playlists yet. Make one, or import one from a link.")
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

struct HistorySection: View {
    @Environment(AppModel.self) private var model
    @State private var found: ListeningHistory.Found?
    @State private var failed = false

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            if let found {
                Text(found.summary).font(Theme.small).foregroundStyle(Theme.soft)
                if let first = found.entries.first {
                    PlayAllButton(label: "Play") { play(found.entries, at: 0, from: first) }
                }
                if !found.enabled {
                    VStack(alignment: .leading, spacing: 4) {
                        Text(found.entries.isEmpty ? "Listening history is paused" : "Listening history is paused, so new plays are not added.")
                            .font(found.entries.isEmpty ? Theme.bodyStrong : Theme.small)
                            .foregroundStyle(found.entries.isEmpty ? Theme.text : Theme.soft)
                        if found.entries.isEmpty {
                            Text("New plays are not being saved. Turn history back on in Settings to see what you play here.").font(Theme.small).foregroundStyle(Theme.soft)
                        }
                        NavigationLink(value: SettingsPage.privacy) {
                            Text("Open listening history settings").font(Theme.smallStrong).foregroundStyle(Theme.focus).frame(minHeight: 44)
                        }
                        .buttonStyle(.plain)
                    }
                } else if found.entries.isEmpty {
                    VStack(alignment: .leading, spacing: 4) {
                        Text("Start listening").font(Theme.bodyStrong)
                        Text("Choose music from your library to get started.").font(Theme.small).foregroundStyle(Theme.soft)
                    }
                }
                VStack(spacing: 0) {
                    ForEach(Array(found.entries.enumerated()), id: \.element.id) { index, entry in
                        HStack(spacing: 0) {
                            Button { play(found.entries, at: index, from: entry) } label: {
                                LibraryRow(track: entry.track, isCurrent: model.player?.current?.id == entry.track.id)
                            }
                            .buttonStyle(.plain)
                            Menu { TrackMenu(track: entry.track) } label: {
                                Icon("more-vertical").foregroundStyle(Theme.quiet).frame(width: 44, height: 44).contentShape(Circle())
                            }
                            .accessibilityLabel("More actions for \(entry.track.title)")
                        }
                    }
                }
                .padding(.leading, -6)
                .padding(.trailing, -8)
            } else if failed {
                HStack(spacing: 8) {
                    Text("History didn't load.").foregroundStyle(Theme.soft)
                    TextAction(title: "Try again") { Task { await load() } }
                }
                .font(Theme.small)
            } else {
                ProgressView().tint(Theme.soft).frame(maxWidth: .infinity, minHeight: 60)
            }
        }
        .task { await load() }
    }

    private func play(_ entries: [ListeningHistory.Entry], at index: Int, from entry: ListeningHistory.Entry) {
        model.player?.play(entries.map(\.track), at: index, from: "Listening history", startingAt: Double(entry.positionMs) / 1000)
    }

    private func load() async {
        guard let api = model.api else { return }
        do {
            found = try await ListeningHistory(api: api).load()
            failed = false
        } catch {
            failed = found == nil
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
                    HStack(spacing: 8) {
                        ZStack {
                            RoundedRectangle(cornerRadius: 4).fill(picked ? Theme.accent : .clear)
                            RoundedRectangle(cornerRadius: 4).strokeBorder(picked ? Theme.accent : Theme.quiet, lineWidth: 1.5)
                            if picked { Icon("check", size: 13).foregroundStyle(Theme.onAccent) }
                        }
                        .frame(width: 20, height: 20)
                        .frame(width: 44, height: 44)
                        LibraryRow(track: track, isCurrent: false)
                    }
                    .background(RoundedRectangle(cornerRadius: 10).fill(picked ? Theme.selected : .clear))
                }
                .buttonStyle(.plain)
                .accessibilityAddTraits(picked ? [.isButton, .isSelected] : .isButton)
            } else {
                HStack(spacing: 0) {
                    Button {
                        model.player?.play(feed.items, at: index, from: context)
                    } label: {
                        LibraryRow(track: track, isCurrent: model.player?.current?.id == track.id, showsTime: track.isPlayable)
                    }
                    .buttonStyle(.plain)
                    if !track.isPlayable {
                        FindButton(track: track) { feed.replace(track.id, with: $0) }
                    }
                }
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
        Button("Play next") { model.player?.playNext(track) }
        NavigationLink("Track details", value: track)
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
                Text(list.title).font(Theme.title).tracking(Theme.titleTracking).accessibilityAddTraits(.isHeader)
                if let total = feed.total, total > 0 {
                    PlayAllButton(label: "Play") {
                        model.player?.play(feed.items, at: 0, from: list.title)
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
    var stretch = false
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            HStack(spacing: 8) {
                Icon("play", size: 16)
                Text(label).font(Theme.bodyStrong).lineLimit(1).minimumScaleFactor(0.8)
            }
            .foregroundStyle(Theme.onAccent)
            .padding(.horizontal, stretch ? 14 : 22)
            .frame(maxWidth: stretch ? .infinity : nil, minHeight: 48)
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
    @State private var edition: EditionRead?
    @State private var onlyMine = false
    @State private var adding: Set<String> = []
    @State private var notes: [String: String] = [:]

    var tracks: [Track] { detail?.tracks ?? [] }

    private var complete: CompleteAlbum? {
        if case let .available(album) = edition { return album }
        return nil
    }

    private var playable: [Track] {
        guard let complete else { return tracks }
        let owned = complete.tracks.compactMap(\.owned)
        let ids = Set(owned.map(\.id))
        return owned + tracks.filter { !ids.contains($0.id) }
    }

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
                if let complete {
                    HStack(spacing: 12) {
                        GeometryReader { box in
                            ZStack(alignment: .leading) {
                                Capsule().fill(Theme.line)
                                Capsule().fill(Theme.text).frame(width: complete.tracks.isEmpty ? 0 : box.size.width * CGFloat(complete.ownedCount) / CGFloat(complete.tracks.count))
                            }
                        }
                        .frame(width: 160, height: 4)
                        .accessibilityHidden(true)
                        Text(complete.summary).font(Theme.small).foregroundStyle(Theme.soft)
                    }
                }
                HStack(spacing: 8) {
                    if !tracks.isEmpty {
                        PlayAllButton(label: "Play") {
                            model.player?.play(playable, at: 0, from: album.title)
                        }
                        if let complete {
                            let missing = complete.addable.filter { notes[$0.id] == nil }
                            if !missing.isEmpty {
                                Button { Task { await addAll(missing, in: complete) } } label: {
                                    HStack(spacing: 8) {
                                        Icon("plus", size: 16)
                                        Text(adding.isEmpty ? "Add the other \(missing.count)" : "Adding…").font(Theme.bodyMedium).lineLimit(1)
                                    }
                                    .foregroundStyle(Theme.text)
                                    .padding(.horizontal, 18)
                                    .frame(minHeight: 48)
                                    .overlay(Capsule().strokeBorder(Theme.line))
                                }
                                .buttonStyle(.plain)
                                .disabled(!adding.isEmpty)
                            }
                        }
                        Menu {
                            Button("Play next") { tracks.forEach { model.player?.playNext($0) } }
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
                if let complete {
                    editionHeader(complete)
                    VStack(spacing: 0) {
                        ForEach(complete.tracks) { track in
                            if let owned = track.owned {
                                ownedRow(owned)
                            } else if !onlyMine {
                                missingRow(track, in: complete)
                            }
                        }
                        let ids = Set(complete.tracks.compactMap(\.owned?.id))
                        ForEach(tracks.filter { !ids.contains($0.id) }) { ownedRow($0) }
                    }
                    .padding(.trailing, -8)
                } else {
                    if case let .unavailable(reason) = edition {
                        Text(reason).font(Theme.small).foregroundStyle(Theme.quiet)
                    }
                    VStack(spacing: 0) {
                        ForEach(tracks) { ownedRow($0) }
                    }
                    .padding(.trailing, -8)
                }
            }
            .padding(.horizontal, 16)
            .padding(.bottom, 16)
        }
        .scrollIndicators(.hidden)
        .task { if detail == nil { await load() } }
    }

    private func editionHeader(_ complete: CompleteAlbum) -> some View {
        Wrap(spacing: 8) {
            FilterChip(title: "All \(complete.tracks.count)", on: !onlyMine) { onlyMine = false }
            FilterChip(title: "In my library \(complete.ownedCount)", on: onlyMine) { onlyMine = true }
            HStack(spacing: 8) {
                Text("Edition").foregroundStyle(Theme.quiet)
                Text(complete.editionName).fontWeight(.medium).lineLimit(1)
            }
            .font(Theme.small)
            .foregroundStyle(Theme.text)
            .padding(.horizontal, 16)
            .frame(minHeight: 44)
            .overlay(Capsule().strokeBorder(Theme.line))
            .accessibilityElement(children: .combine)
        }
    }

    private func ownedRow(_ track: Track) -> some View {
        HStack(spacing: 4) {
            Button {
                let list = playable
                model.player?.play(list, at: list.firstIndex { $0.id == track.id } ?? 0, from: album.title)
            } label: {
                AlbumTrackRow(track: track, isCurrent: model.player?.current?.id == track.id)
            }
            .buttonStyle(.plain)
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

    private func missingRow(_ track: EditionTrack, in complete: CompleteAlbum) -> some View {
        let canAdd = complete.addable.contains { $0.id == track.id } && notes[track.id] == nil
        return HStack(spacing: 4) {
            VStack(alignment: .leading, spacing: 0) {
                Text(track.title).foregroundStyle(Theme.soft).lineLimit(1)
                HStack(spacing: 6) {
                    let finding = adding.contains(track.id)
                    Circle()
                        .strokeBorder(finding ? Theme.quiet : Theme.missingRing, style: StrokeStyle(lineWidth: 1.5, dash: finding ? [2, 2] : []))
                        .frame(width: 9, height: 9)
                    Text(finding ? "Finding audio…" : notes[track.id] ?? track.note ?? "")
                        .foregroundStyle(finding ? Theme.quiet : Theme.missing)
                        .lineLimit(1)
                }
                .font(Theme.small)
            }
            .frame(maxWidth: .infinity, minHeight: 60, alignment: .leading)
            if adding.contains(track.id) {
                ProgressView().tint(Theme.soft).frame(width: 44, height: 44)
            } else if canAdd {
                Button { Task { await add(track, in: complete) } } label: {
                    Icon("plus", size: 16).foregroundStyle(Theme.soft).frame(width: 44, height: 44).contentShape(Circle())
                }
                .buttonStyle(.plain)
                .disabled(!adding.isEmpty)
                .accessibilityLabel("Add \(track.title) to your library")
            }
        }
        .accessibilityElement(children: .combine)
    }

    private func add(_ track: EditionTrack, in complete: CompleteAlbum) async {
        guard let api = model.api, let saved = detail?.savedEdition else { return }
        adding.insert(track.id)
        defer { adding.remove(track.id) }
        do {
            switch try await AlbumEditions(api: api).add(track, of: complete, release: album.id, saved: saved) {
            case let .saved(recording):
                if case let .available(current) = edition { edition = .available(current.replacing(track.id, with: recording)) }
            case let .missing(reason):
                notes[track.id] = reason
            }
        } catch {
            notes[track.id] = "Couldn't find audio"
            model.handle(error)
        }
    }

    private func addAll(_ missing: [EditionTrack], in complete: CompleteAlbum) async {
        for track in missing {
            guard case let .available(current) = edition else { return }
            await add(track, in: current)
        }
    }

    private var facts: String {
        let shown = detail?.album ?? album
        let count = "\(shown.trackCount) \(shown.trackCount == 1 ? "track" : "tracks")"
        let length = shown.durationMs.flatMap { $0 >= 60_000 ? Duration.milliseconds($0).long : nil }
        return [shown.credit, count, length].compactMap { $0 }.joined(separator: " · ")
    }

    private func load() async {
        guard let api = model.api else { return }
        failed = false
        do {
            detail = try await Library(api: api).album(album.id)
        } catch {
            failed = true
            model.handle(error)
            return
        }
        guard detail?.savedEdition != nil else { return }
        edition = try? await AlbumEditions(api: api).read(album.id)
    }
}

enum TrackShown: Hashable, CaseIterable {
    case all, ready, noSource

    var availability: Availability? {
        switch self {
        case .all: nil
        case .ready: .playable
        case .noSource: .noSource
        }
    }

    var label: String { availability?.label ?? "All" }
}

struct FilterSheet: View {
    @Environment(\.dismiss) private var dismiss
    @Binding var sort: TrackSort
    @Binding var shown: TrackShown
    let filters: Bool
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
            if filters {
                VStack(alignment: .leading, spacing: 8) {
                    Text("Availability").font(Theme.small).foregroundStyle(Theme.quiet)
                    FlowChips(items: TrackShown.allCases, label: \.label, selected: shown) { shown = $0 }
                }
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
                TextAction(title: "Reset") {
                    sort = .recent
                    shown = .all
                }
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

struct ActiveChip: View {
    let title: String
    let clear: () -> Void

    var body: some View {
        Button(action: clear) {
            HStack(spacing: 8) {
                Text(title).font(Theme.smallMedium).lineLimit(1)
                Icon("close", size: 14)
            }
            .foregroundStyle(Theme.ground)
            .padding(.leading, 14)
            .padding(.trailing, 12)
            .frame(minHeight: 44)
            .background(Capsule().fill(Theme.text))
        }
        .buttonStyle(.plain)
        .accessibilityLabel("Remove \(title)")
    }
}

struct FlowChips<Item: Hashable>: View {
    let items: [Item]
    let label: KeyPath<Item, String>
    let selected: Item
    let pick: (Item) -> Void

    var body: some View {
        Wrap(spacing: 8) { chips }
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

struct Wrap: Layout {
    var spacing: CGFloat

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let rows = arrange(width: proposal.width ?? .infinity, subviews: subviews)
        let width = rows.map { $0.width }.max() ?? 0
        let height = rows.map(\.height).reduce(0, +) + spacing * CGFloat(max(rows.count - 1, 0))
        return CGSize(width: proposal.width ?? width, height: height)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        var y = bounds.minY
        for row in arrange(width: bounds.width, subviews: subviews) {
            var x = bounds.minX
            for index in row.indices {
                let size = subviews[index].sizeThatFits(.unspecified)
                subviews[index].place(at: CGPoint(x: x, y: y), proposal: ProposedViewSize(width: min(size.width, bounds.width), height: size.height))
                x += size.width + spacing
            }
            y += row.height + spacing
        }
    }

    private struct Row {
        var indices: [Int] = []
        var width: CGFloat = 0
        var height: CGFloat = 0
    }

    private func arrange(width: CGFloat, subviews: Subviews) -> [Row] {
        var rows: [Row] = []
        var row = Row()
        for index in subviews.indices {
            let size = subviews[index].sizeThatFits(.unspecified)
            let needed = row.indices.isEmpty ? size.width : row.width + spacing + size.width
            if needed > width, !row.indices.isEmpty {
                rows.append(row)
                row = Row()
            }
            row.width = row.indices.isEmpty ? size.width : row.width + spacing + size.width
            row.height = max(row.height, size.height)
            row.indices.append(index)
        }
        if !row.indices.isEmpty { rows.append(row) }
        return rows
    }
}

struct SelectionBar: View {
    let count: Int
    let loaded: Int
    let playlists: [Playlist]
    let finding: String?
    let selectLoaded: () -> Void
    let find: () -> Void
    let queue: () -> Void
    let add: (Playlist) -> Void
    let close: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 8) {
                Text("\(count) selected").font(Theme.bodyBold)
                Spacer(minLength: 0)
                if count < loaded {
                    TextAction(title: "Select the \(loaded) loaded", color: Theme.selectedAction, inset: 12, action: selectLoaded)
                }
                IconButton(icon: "close", label: "Stop selecting", glyph: 18, action: close)
            }
            HStack(spacing: 8) {
                Button(action: find) {
                    Text(finding ?? "Find sources")
                        .font(Theme.smallStrong)
                        .lineLimit(1)
                        .minimumScaleFactor(0.8)
                        .foregroundStyle(Theme.onAccent)
                        .frame(maxWidth: .infinity, minHeight: 44)
                        .background(Capsule().fill(Theme.accent).opacity(finding == nil ? 1 : 0.6))
                }
                .buttonStyle(.plain)
                .disabled(count == 0 || finding != nil)
                Menu {
                    if playlists.isEmpty {
                        Text("No playlists of yours yet")
                    }
                    ForEach(playlists) { playlist in
                        Button(playlist.name) { add(playlist) }
                    }
                } label: {
                    Text("Add to playlist")
                        .font(Theme.smallMedium)
                        .lineLimit(1)
                        .minimumScaleFactor(0.8)
                        .foregroundStyle(Theme.text)
                        .frame(maxWidth: .infinity, minHeight: 44)
                        .overlay(Capsule().strokeBorder(Theme.text.opacity(0.35)))
                }
                .disabled(count == 0)
                Menu {
                    Button("Add to queue", action: queue)
                } label: {
                    Icon("more").foregroundStyle(Theme.text).frame(width: 44, height: 44).contentShape(Circle())
                }
                .disabled(count == 0)
                .accessibilityLabel("More actions for the selected tracks")
            }
        }
        .padding(EdgeInsets(top: 8, leading: 16, bottom: 8, trailing: 8))
        .background(RoundedRectangle(cornerRadius: 16).fill(Theme.selected))
        .padding(.horizontal, 8)
        .padding(.bottom, 8)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Selection actions")
    }
}
