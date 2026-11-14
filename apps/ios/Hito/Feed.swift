import HitoKit
import SwiftUI

@MainActor
@Observable
final class Feed<Item: Decodable & Equatable & Identifiable & Sendable> {
    typealias Fetch = @Sendable (Int) async throws -> Page<Item>

    private(set) var items: [Item] = []
    private(set) var total: Int?
    private(set) var loading = false
    private(set) var failed = false
    private(set) var unsupported: String?
    private var fetch: Fetch?
    private var generation = 0

    var isEmpty: Bool { total == 0 && unsupported == nil }
    var hasMore: Bool { total.map { items.count < $0 } ?? true }

    func replace(_ id: Item.ID, with item: Item) {
        guard let index = items.firstIndex(where: { $0.id == id }) else { return }
        items[index] = item
    }

    func reset(_ fetch: @escaping Fetch) {
        generation += 1
        self.fetch = fetch
        items = []
        total = nil
        failed = false
        unsupported = nil
        loading = false
    }

    func more(_ model: AppModel) async {
        guard let fetch, !loading, hasMore else { return }
        loading = true
        failed = false
        let offset = items.count
        let current = generation
        do {
            let page = try await fetch(offset)
            guard current == generation else { return }
            items += page.items
            total = page.total
        } catch {
            guard current == generation else { return }
            if let refused = error as? TrackSort.Unsupported {
                unsupported = refused.message
            } else if let refused = error as? Availability.Unconfirmed {
                unsupported = refused.message
                total = items.count
            } else if !(error is CancellationError) {
                failed = true
                model.handle(error)
            }
        }
        loading = false
    }
}

struct FeedFooter<Item: Decodable & Equatable & Identifiable & Sendable>: View {
    @Environment(AppModel.self) private var model
    let feed: Feed<Item>

    var body: some View {
        if let unsupported = feed.unsupported {
            Text(unsupported).font(Theme.small).foregroundStyle(Theme.soft)
        } else if feed.failed {
            HStack(spacing: 8) {
                Text("This didn't load.").foregroundStyle(Theme.soft)
                TextAction(title: "Try again") { Task { await feed.more(model) } }
            }
            .font(Theme.small)
        } else if feed.hasMore {
            ProgressView()
                .tint(Theme.soft)
                .frame(maxWidth: .infinity, minHeight: 60)
                .task { await feed.more(model) }
        }
    }
}

enum TrackList: Hashable {
    case liked
    case artist(Artist)

    var title: String {
        switch self {
        case .liked: "Liked tracks"
        case .artist(let artist): artist.name
        }
    }

    var empty: String {
        switch self {
        case .liked: "Tracks you like show up here."
        case .artist: "No tracks by this artist in your library."
        }
    }

    func page(_ library: Library, _ offset: Int) async throws -> Page<Track> {
        switch self {
        case .liked: try await library.tracks(offset: offset, liked: true)
        case .artist(let artist): try await library.tracks(offset: offset, artist: artist.id)
        }
    }
}

extension Track {
    var readied: Track {
        Track(id: id, title: title, artist: artist, album: album, releaseId: releaseId, durationMs: durationMs, available: true, playsFromServices: playsFromServices, liked: liked, trackNumber: trackNumber)
    }
}

struct FindButton: View {
    @Environment(AppModel.self) private var model
    let track: Track
    let found: (Track) -> Void
    @State private var finding = false
    @State private var note: String?
    @State private var request = UUID()

    var body: some View {
        Group {
            if finding {
                ProgressView().tint(Theme.soft).frame(width: 52, height: 44)
            } else if let note {
                Text(note).font(Theme.small).foregroundStyle(Theme.quiet).lineLimit(2).multilineTextAlignment(.trailing).frame(width: 110, alignment: .trailing).padding(.trailing, 8)
            } else {
                Button { Task { await find() } } label: {
                    Text("Find").font(Theme.smallMedium).foregroundStyle(Theme.focus).padding(.horizontal, 8).frame(minHeight: 44)
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Find a source for \(track.title)")
            }
        }
    }

    private func find() async {
        guard let api = model.api else { return }
        finding = true
        defer { finding = false }
        do {
            switch try await Playback(api: api).find(track.id, request: request) {
            case .found: found(track.readied)
            case let .notFound(reason): note = reason
            }
            request = UUID()
        } catch {
            note = "Couldn't search"
            model.handle(error)
        }
    }
}
