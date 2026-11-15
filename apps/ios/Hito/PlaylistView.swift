import HitoKit
import SwiftUI

struct PlaylistView: View {
    enum Filter: Hashable {
        case all, ready, notReady
    }

    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @Environment(\.dynamicTypeSize) private var typeSize
    let playlist: Playlist
    @State private var detail: PlaylistDetail?
    @State private var failed = false
    @State private var filter = Filter.all
    @State private var tracks: [Track] = []
    @State private var finding: (done: Int, total: Int)?
    @State private var findNote: String?
    @State private var search: Task<Void, Never>?
    @State private var draft: PlaylistDraft?
    @State private var saving = false
    @State private var editProblem: String?
    @State private var confirmingDelete = false

    var ready: [Track] { tracks.filter(\.isPlayable) }
    var notReady: [Track] { tracks.filter { !$0.isPlayable } }
    var findable: [Track] { tracks.filter(\.canFind) }

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
                    }
                    if playlist.kind == .followed, !failed {
                        PlaylistFollowCard(playlist: playlist.id) { await load() }
                    }
                    if failed || detail == nil {
                        EmptyView()
                    } else if let draft {
                        editor(draft)
                    } else if tracks.isEmpty {
                        Text("No tracks in this playlist yet.").font(Theme.small).foregroundStyle(Theme.soft)
                    } else {
                        if let editProblem {
                            Text(editProblem).font(Theme.small).foregroundStyle(Theme.warning)
                        }
                        chips.padding(.bottom, -8)
                        VStack(spacing: 0) {
                            ForEach(shown) { track in
                                HStack(spacing: 12) {
                                    Button {
                                        model.player?.play(shown, at: shown.firstIndex(of: track) ?? 0, from: playlist.name)
                                    } label: {
                                        LibraryRow(track: track, isCurrent: model.player?.current?.id == track.id, showsTime: false, gap: 12, dimsUnavailable: true)
                                    }
                                    .buttonStyle(.plain)
                                    Menu {
                                        TrackMenu(track: track)
                                        if playlist.kind == .local, let entry = entry(for: track) {
                                            Button("Remove from playlist", role: .destructive) { Task { await remove(entry) } }
                                        }
                                    } label: {
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
                .padding(.top, 12)
                .padding(.bottom, 16)
            }
        }
        .scrollIndicators(.hidden)
        .task { if detail == nil { await load() } }
        .onDisappear { search?.cancel() }
        .confirmationDialog("Delete \(detail?.playlist.name ?? playlist.name)?", isPresented: $confirmingDelete, titleVisibility: .visible) {
            Button("Delete playlist", role: .destructive) { Task { await deletePlaylist() } }
        } message: {
            Text(playlist.kind == .followed ? "The playlist and its updates are removed from your account. The tracks stay in your library." : "The playlist is removed from your account. The tracks stay in your library.")
        }
    }

    private var header: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack(alignment: .center, spacing: 14) {
                PlaylistCover(playlist: playlist, cover: detail?.cover ?? .playlist, size: 96)
                VStack(alignment: .leading, spacing: 4) {
                    Text(detail?.origin ?? (playlist.kind == .followed ? "Followed playlist" : "Your playlist"))
                        .font(Theme.label)
                        .foregroundStyle(Theme.soft)
                        .lineLimit(1)
                    Text(detail?.playlist.name ?? playlist.name)
                        .font(Theme.display).tracking(Theme.titleTracking)
                        .lineLimit(3)
                        .accessibilityAddTraits(.isHeader)
                    if detail != nil {
                        Text("\(ready.count) of \(tracks.count) ready").font(Theme.small).foregroundStyle(Theme.text)
                    }
                }
            }
            if !tracks.isEmpty {
                GeometryReader { proxy in
                    ZStack(alignment: .leading) {
                        Capsule().fill(Theme.text.opacity(0.2))
                        Capsule().fill(Theme.text).frame(width: proxy.size.width * CGFloat(ready.count) / CGFloat(tracks.count))
                    }
                }
                .frame(height: 6)
                .accessibilityHidden(true)
            }
            if tracks.isEmpty, detail != nil, draft == nil {
                moreMenu
            }
            if !tracks.isEmpty, draft == nil {
                let actions = typeSize.isAccessibilitySize ? AnyLayout(VStackLayout(alignment: .leading, spacing: 8)) : AnyLayout(HStackLayout(spacing: 8))
                actions {
                    if !ready.isEmpty {
                        PlayAllButton(label: "Play \(ready.count) ready", stretch: true) {
                            model.player?.play(ready, at: 0, from: playlist.name)
                        }
                    }
                    if let finding {
                        OutlineAction(title: "Finding \(min(finding.done + 1, finding.total)) of \(finding.total)…", busy: true) { search?.cancel() }
                            .accessibilityHint("Stops looking for sources")
                    } else if !findable.isEmpty {
                        OutlineAction(title: "Find \(findable.count)", busy: false, action: findAll)
                            .accessibilityLabel("Find sources for \(findable.count) tracks that aren't ready")
                    }
                    moreMenu
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
        .background(Theme.header.ignoresSafeArea(edges: .top))
    }

    @ViewBuilder private var moreMenu: some View {
        if !ready.isEmpty || detail != nil {
            Menu {
                if !ready.isEmpty {
                    Button("Play next") { ready.forEach { model.player?.playNext($0) } }
                }
                if playlist.kind == .local, let detail {
                    Button("Edit playlist") {
                        draft = PlaylistDraft(name: detail.playlist.name, entries: detail.entries)
                        editProblem = nil
                    }
                }
                if detail != nil {
                    Button("Delete playlist", role: .destructive) { confirmingDelete = true }
                }
            } label: {
                Icon("more").foregroundStyle(Theme.text).frame(width: 48, height: 48).contentShape(Circle())
            }
            .accessibilityLabel("More actions")
        }
    }

    private func editor(_ current: PlaylistDraft) -> some View {
        let byEntry = Dictionary(uniqueKeysWithValues: zip((detail?.entries ?? []).map(\.id), tracks))
        let unavailable = current.entries.filter { byEntry[$0]?.isPlayable == false }
        return VStack(alignment: .leading, spacing: 12) {
            Text("Edit playlist").font(Theme.section).accessibilityAddTraits(.isHeader)
            TextField("Playlist name", text: Binding(get: { draft?.name ?? "" }, set: { draft?.name = $0 }))
                .padding(.horizontal, 14)
                .frame(minHeight: 44)
                .background(RoundedRectangle(cornerRadius: 10).fill(Theme.field))
            if !unavailable.isEmpty {
                TextAction(title: "Remove \(unavailable.count) without a source") { unavailable.forEach { draft?.remove($0) } }
            }
            ForEach(current.entries, id: \.self) { entry in
                HStack(spacing: 4) {
                    if let track = byEntry[entry] {
                        LibraryRow(track: track, showsTime: false)
                    }
                    Button { shift(entry, by: -1) } label: {
                        Image(systemName: "chevron.up").frame(width: 36, height: 44)
                    }
                    .disabled(current.entries.first == entry)
                    .accessibilityLabel("Move up")
                    Button { shift(entry, by: 1) } label: {
                        Image(systemName: "chevron.down").frame(width: 36, height: 44)
                    }
                    .disabled(current.entries.last == entry)
                    .accessibilityLabel("Move down")
                    Button { draft?.remove(entry) } label: {
                        Icon("close", size: 16).frame(width: 36, height: 44)
                    }
                    .accessibilityLabel("Remove \(byEntry[entry]?.title ?? "track") from playlist")
                }
                .buttonStyle(.plain)
                .foregroundStyle(Theme.soft)
            }
            if let editProblem { Text(editProblem).font(Theme.small).foregroundStyle(Theme.warning) }
            HStack(spacing: 8) {
                Button("Discard changes") { draft = nil; editProblem = nil }
                    .buttonStyle(.plain)
                    .font(Theme.bodyMedium)
                    .padding(.horizontal, 18)
                    .frame(minHeight: 48)
                    .overlay(Capsule().strokeBorder(Theme.line))
                    .disabled(saving)
                PrimaryButton(title: "Save changes", busyTitle: "Saving…", busy: saving) { Task { await save(current) } }
                    .disabled(saving || current.name.trimmingCharacters(in: .whitespaces).isEmpty)
            }
        }
    }

    private func save(_ current: PlaylistDraft) async {
        guard let api = model.api else { return }
        let changes = current.changes
        guard !changes.isEmpty else { draft = nil; return }
        saving = true
        defer { saving = false }
        do {
            try await Playlists(api: api).apply(changes, to: playlist.id, device: Player.device)
            draft = nil
            editProblem = nil
            await load()
        } catch {
            editProblem = "The changes weren't saved. Try again."
        }
    }

    private func entry(for track: Track) -> String? {
        guard let entries = detail?.entries, let index = tracks.firstIndex(where: { $0.id == track.id }), entries.indices.contains(index) else { return nil }
        return entries[index].id
    }

    private func remove(_ entry: String) async {
        guard let api = model.api else { return }
        do {
            try await Playlists(api: api).apply([.remove(entry)], to: playlist.id, device: Player.device)
            await load()
        } catch {
            editProblem = "The track wasn't removed. Try again."
            model.handle(error)
        }
    }

    private func shift(_ entry: String, by delta: Int) {
        if draft?.move(entry, by: delta) == false {
            editProblem = "This playlist's order can't be changed here. Try again later."
        }
    }

    private func deletePlaylist() async {
        guard let api = model.api else { return }
        do {
            try await Playlists(api: api).delete(playlist.id, device: Player.device)
            dismiss()
        } catch {
            editProblem = "The playlist wasn't deleted. Try again."
        }
    }

    private var chips: some View {
        Wrap(spacing: 8) {
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
        let ids = findable.map(\.id)
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
                Text(title).font(Theme.bodyMedium).lineLimit(1).minimumScaleFactor(0.8)
            }
            .foregroundStyle(Theme.text)
            .padding(.horizontal, 16)
            .frame(maxWidth: .infinity, minHeight: 48)
            .overlay(Capsule().strokeBorder(Theme.text.opacity(0.4)))
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
                .font(Theme.smallMedium)
                .lineLimit(1)
                .fixedSize()
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

struct PlaylistCover: View {
    let playlist: Playlist
    let cover: PlaylistDetail.Cover
    let size: CGFloat

    var body: some View {
        switch cover {
        case .playlist:
            Cover(path: playlist.artworkPath, size: size, radius: 10, mark: "playlist")
        case let .track(id):
            Cover(path: ["api", "v1", "recordings", id, "artwork"], size: size, radius: 10, mark: "playlist")
        case let .mosaic(ids):
            Grid(horizontalSpacing: 0, verticalSpacing: 0) {
                GridRow {
                    tile(ids[0])
                    tile(ids[1])
                }
                GridRow {
                    tile(ids[2])
                    tile(ids[3])
                }
            }
            .frame(width: size, height: size)
            .clipShape(RoundedRectangle(cornerRadius: 10))
            .accessibilityHidden(true)
        }
    }

    private func tile(_ id: String) -> some View {
        Cover(path: ["api", "v1", "recordings", id, "artwork"], size: size / 2, radius: 0)
    }
}
