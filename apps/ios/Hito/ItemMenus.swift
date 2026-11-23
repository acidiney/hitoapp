import HitoKit
import SwiftUI

struct ActionItems<Destination: Hashable>: View {
    let actions: [ItemAction]
    let destination: (ItemAction) -> Destination?
    let perform: (ItemAction) -> Void

    var body: some View {
        ForEach(actions, id: \.self) { action in
            if let value = destination(action) {
                NavigationLink(action.title, value: value)
            } else {
                Button(action.title, role: action.isDestructive ? .destructive : nil) { perform(action) }
            }
        }
    }
}

extension ActionItems where Destination == Never {
    init(actions: [ItemAction], perform: @escaping (ItemAction) -> Void) {
        self.init(actions: actions, destination: { _ in nil }, perform: perform)
    }
}

extension ArtworkRetries {
    func isMissing(_ path: [String]) -> Bool {
        ArtworkRetrieval.Subject(path: path).map(unavailable.contains) ?? false
    }

    func retry(_ path: [String]) {
        if let subject = ArtworkRetrieval.Subject(path: path) { retry(subject) }
    }
}

struct TrackMenu: View {
    @Environment(AppModel.self) private var model
    let track: Track
    var extra: [ItemAction] = []
    var perform: (ItemAction) -> Void = { _ in }

    var body: some View {
        let retries = ArtworkRetries.shared
        ActionItems(
            actions: ItemMenu.track(track, liked: model.player?.isLiked(track), artworkMissing: retries.isMissing(track.artworkPath)) + extra,
            destination: { [.details, .findAudio, .fixSource].contains($0) ? track : nil }
        ) { action in
            switch action {
            case .playNext: model.player?.playNext(track)
            case .like, .unlike: model.player?.toggleLike(track)
            case .retryArtwork: retries.retry(track.artworkPath)
            default: perform(action)
            }
        }
    }
}

struct AlbumMenu: View {
    let album: Album
    let perform: (ItemAction) -> Void

    var body: some View {
        ActionItems(actions: ItemMenu.album(artworkMissing: ArtworkRetries.shared.isMissing(album.artworkPath)), perform: perform)
    }
}

struct PlaylistEditing: Hashable {
    let playlist: Playlist
}

struct PlaylistMenu: View {
    let playlist: Playlist
    let ready: Int?
    var editsHere = false
    let perform: (ItemAction) -> Void

    var body: some View {
        ActionItems(
            actions: ItemMenu.playlist(playlist, ready: ready, artworkMissing: ArtworkRetries.shared.isMissing(playlist.artworkPath)),
            destination: { $0 == .edit && !editsHere ? PlaylistEditing(playlist: playlist) : nil },
            perform: perform
        )
    }

    static func deleteMessage(_ playlist: Playlist) -> String {
        playlist.kind == .followed
            ? "The playlist and its updates are removed from your account. The tracks stay in your library."
            : "The playlist is removed from your account. The tracks stay in your library."
    }
}

struct QueueMenu: View {
    let track: Track
    let player: Player
    let among: [Track]

    var body: some View {
        ForEach(ItemMenu.queued, id: \.self) { action in
            Button(action.title, role: action.isDestructive ? .destructive : nil) {
                switch action {
                case .moveUp: player.move(upcoming: track.id, by: -1)
                case .moveDown: player.move(upcoming: track.id, by: 1)
                default: player.remove(upcoming: track)
                }
            }
            .disabled(action == .moveUp ? among.first?.id == track.id : action == .moveDown ? among.last?.id == track.id : false)
        }
    }
}

struct ItemPreview: View {
    let path: [String]?
    let title: String
    let detail: String?
    var mark = "note"
    var round = false

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            if let path {
                Cover(path: path, size: 248, radius: 12, mark: mark, round: round)
            } else {
                ZStack { Theme.tile; Icon(mark, size: 72).foregroundStyle(Theme.coverMark) }
                    .frame(width: 248, height: 248)
                    .clipShape(round ? AnyShape(Circle()) : AnyShape(RoundedRectangle(cornerRadius: 12)))
            }
            VStack(alignment: .leading, spacing: 2) {
                Text(title).font(Theme.bodyStrong).lineLimit(2)
                if let detail {
                    Text(detail).font(Theme.small).foregroundStyle(Theme.quiet).lineLimit(1)
                }
            }
        }
        .padding(16)
        .frame(width: 280, alignment: .leading)
        .background(Theme.field)
        .foregroundStyle(Theme.text)
    }
}

extension View {
    func trackActions(_ track: Track, extra: [ItemAction] = [], perform: @escaping (ItemAction) -> Void = { _ in }) -> some View {
        modifier(TrackActions(track: track, extra: extra, perform: perform))
    }

    func albumActions(_ album: Album) -> some View {
        modifier(AlbumActions(album: album))
    }

    func playlistActions(_ playlist: Playlist, changed: @escaping () -> Void = {}) -> some View {
        modifier(PlaylistActions(playlist: playlist, changed: changed))
    }

    func collectionActions(_ title: String, preview: ItemPreview, tracks: @escaping (HitoAPI) async throws -> [Track]) -> some View {
        modifier(CollectionActions(title: title, preview: preview, tracks: tracks))
    }

    func artistActions(_ artist: Artist, detail: String) -> some View {
        collectionActions(artist.name, preview: ItemPreview(path: nil, title: artist.name, detail: detail, mark: "person", round: true)) {
            try await ArtistPage(api: $0).load(artist.id).tracks
        }
    }

    func problemAlert(_ problem: Binding<String?>) -> some View {
        alert(problem.wrappedValue ?? "", isPresented: Binding(get: { problem.wrappedValue != nil }, set: { if !$0 { problem.wrappedValue = nil } })) {
            Button("OK", role: .cancel) {}
        }
    }
}

struct TrackActions: ViewModifier {
    @Environment(AppModel.self) private var model
    let track: Track
    let extra: [ItemAction]
    let perform: (ItemAction) -> Void

    func body(content: Content) -> some View {
        content.contextMenu {
            TrackMenu(track: track, extra: extra, perform: perform)
        } preview: {
            ItemPreview(path: track.artworkPath, title: track.title, detail: track.artist).environment(model)
        }
    }
}

struct AlbumActions: ViewModifier {
    struct ArtistChoice: Identifiable {
        let id: String
        let credit: String?
    }

    @Environment(AppModel.self) private var model
    let album: Album
    @State private var choosing: ArtistChoice?
    @State private var problem: String?

    func body(content: Content) -> some View {
        content
            .contextMenu {
                AlbumMenu(album: album) { action in Task { await perform(action) } }
            } preview: {
                ItemPreview(path: album.artworkPath, title: album.title, detail: album.credit).environment(model)
            }
            .fullScreenCover(item: $choosing) { choice in
                ArtistSheet(recording: choice.id, credit: choice.credit) {}
                    .presentationBackground(.clear)
            }
            .problemAlert($problem)
    }

    private func perform(_ action: ItemAction) async {
        if action == .retryArtwork { return ArtworkRetries.shared.retry(album.artworkPath) }
        guard let api = model.api else { return }
        do {
            let detail = try await Library(api: api).album(album.id)
            guard let first = detail.tracks.first else {
                problem = "\(album.title) has no tracks in your library."
                return
            }
            switch action {
            case .play: model.player?.play(detail.tracks, at: 0, from: album.title)
            case .playNext: detail.tracks.forEach { model.player?.playNext($0) }
            case .chooseArtist: choosing = ArtistChoice(id: first.id, credit: detail.album.credit)
            default: break
            }
        } catch is CancellationError {
        } catch {
            problem = "\(album.title) didn't load. Try again."
            model.handle(error)
        }
    }
}

struct PlaylistActions: ViewModifier {
    @Environment(AppModel.self) private var model
    let playlist: Playlist
    let changed: () -> Void
    @State private var confirmingDelete = false
    @State private var problem: String?

    func body(content: Content) -> some View {
        content
            .contextMenu {
                PlaylistMenu(playlist: playlist, ready: nil) { action in Task { await perform(action) } }
            } preview: {
                ItemPreview(path: playlist.artworkPath, title: playlist.name, detail: playlist.kind == .followed ? "Followed playlist" : "Playlist", mark: "playlist").environment(model)
            }
            .confirmationDialog("Delete \(playlist.name)?", isPresented: $confirmingDelete, titleVisibility: .visible) {
                Button("Delete playlist", role: .destructive) { Task { await delete() } }
            } message: {
                Text(PlaylistMenu.deleteMessage(playlist))
            }
            .problemAlert($problem)
    }

    private func perform(_ action: ItemAction) async {
        switch action {
        case .retryArtwork: ArtworkRetries.shared.retry(playlist.artworkPath)
        case .delete: confirmingDelete = true
        case .play, .playNext: await play(next: action == .playNext)
        default: break
        }
    }

    private func play(next: Bool) async {
        guard let api = model.api else { return }
        do {
            let ready = try await Playlists(api: api).detail(playlist.id).tracks.filter(\.isPlayable)
            guard !ready.isEmpty else {
                problem = "No tracks in \(playlist.name) are ready to play yet."
                return
            }
            if next {
                ready.forEach { model.player?.playNext($0) }
            } else {
                model.player?.play(ready, at: 0, from: playlist.name)
            }
        } catch is CancellationError {
        } catch {
            problem = "\(playlist.name) didn't load. Try again."
            model.handle(error)
        }
    }

    private func delete() async {
        guard let api = model.api else { return }
        do {
            try await Playlists(api: api).delete(playlist.id, device: Player.device)
            changed()
        } catch {
            problem = "The playlist wasn't deleted. Try again."
            model.handle(error)
        }
    }
}

struct CollectionActions: ViewModifier {
    @Environment(AppModel.self) private var model
    let title: String
    let preview: ItemPreview
    let tracks: (HitoAPI) async throws -> [Track]
    @State private var problem: String?

    func body(content: Content) -> some View {
        content
            .contextMenu {
                ActionItems(actions: ItemMenu.collection) { action in Task { await play(shuffled: action == .shuffle) } }
            } preview: {
                preview.environment(model)
            }
            .problemAlert($problem)
    }

    private func play(shuffled: Bool) async {
        guard let api = model.api else { return }
        do {
            let found = try await tracks(api)
            guard !found.isEmpty else {
                problem = "\(title) has no tracks to play."
                return
            }
            model.player?.play(shuffled ? found.shuffled() : found, at: 0, from: title)
        } catch is CancellationError {
        } catch {
            problem = "\(title) didn't load. Try again."
            model.handle(error)
        }
    }
}
