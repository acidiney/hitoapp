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
    private var fetch: Fetch?
    private var generation = 0

    var isEmpty: Bool { total == 0 }
    var hasMore: Bool { total.map { items.count < $0 } ?? true }

    func reset(_ fetch: @escaping Fetch) {
        generation += 1
        self.fetch = fetch
        items = []
        total = nil
        failed = false
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
            if !(error is CancellationError) {
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
        if feed.failed {
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

    var title: String { "Liked tracks" }
}
