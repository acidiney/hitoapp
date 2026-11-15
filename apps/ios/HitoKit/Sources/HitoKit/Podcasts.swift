import Foundation

public struct Podcasts: Sendable {
    public enum Problem: Error, Equatable {
        case rejected
        case mismatched
    }

    public struct Subscription: Decodable, Equatable, Hashable, Identifiable, Sendable {
        public let id: String
        public let title: String?
        public let refreshPending: Bool
        public let refreshFailed: Bool
        let lastRefreshedAt: Int?

        public var name: String { title.flatMap { $0.isEmpty ? nil : $0 } ?? "Untitled podcast" }

        public var status: String? {
            if refreshPending { return "Checking feed…" }
            if refreshFailed { return "Last feed check failed" }
            if lastRefreshedAt == nil { return "Not checked yet" }
            return nil
        }

        enum CodingKeys: String, CodingKey {
            case title
            case id = "subscription_id"
            case refreshPending = "refresh_pending"
            case refreshFailed = "refresh_failed"
            case lastRefreshedAt = "last_refreshed_at"
        }
    }

    public enum State: Equatable, Sendable {
        case new(String)
        case remaining(String, Double)
        case played

        public var label: String {
            switch self {
            case let .new(label), let .remaining(label, _): label
            case .played: "Played"
            }
        }
    }

    public struct Episode: Equatable, Identifiable, Sendable {
        public let track: Track
        public let positionMs: Int

        public var id: String { track.id }

        public var state: State {
            let duration = track.durationMs
            if positionMs <= 0 { return .new(duration > 0 ? "New · \(Self.long(duration))" : "New") }
            if duration > 0, positionMs >= duration - 1500 { return .played }
            let left = duration > 0 ? duration - positionMs : 0
            return .remaining(left > 0 ? "\(Self.long(left)) left" : "In progress", duration > 0 ? Double(positionMs) / Double(duration) : 0)
        }

        static func long(_ milliseconds: Int) -> String {
            let minutes = max(0, Int((Double(milliseconds) / 60000).rounded()))
            return minutes >= 60 ? "\(minutes / 60) h \(minutes % 60) min" : "\(minutes) min"
        }
    }

    public struct EpisodePage: Equatable, Sendable {
        public let episodes: [Episode]
        public let next: String?
        public let total: Int

        public var lead: Episode? {
            episodes.first { if case .remaining = $0.state { return true }; return false } ?? episodes.first
        }

        public var leadLabel: String? {
            guard let lead else { return nil }
            if case .remaining = lead.state { return "Continue · \(lead.state.label)" }
            return "Play latest"
        }
    }

    struct Wire: Decodable {
        struct Item: Decodable { let recording: Track }
        let items: [Item]
        let total: Int
        let nextCursor: String?

        enum CodingKeys: String, CodingKey {
            case items, total
            case nextCursor = "next_cursor"
        }
    }

    struct Saved: Decodable {
        let recordingId: String
        let positionMs: Int

        enum CodingKeys: String, CodingKey {
            case recordingId = "recording_id"
            case positionMs = "position_ms"
        }
    }

    struct SavedRequest: Encodable {
        let recordingIds: [String]

        enum CodingKeys: String, CodingKey { case recordingIds = "recording_ids" }
    }

    struct Subscribe: Encodable {
        let requestId: String
        let feedUrl: String

        enum CodingKeys: String, CodingKey {
            case requestId = "request_id"
            case feedUrl = "feed_url"
        }
    }

    struct Command: Encodable {
        let requestId: String

        enum CodingKeys: String, CodingKey { case requestId = "request_id" }
    }

    struct Receipt: Decodable {
        let requestId: String
        let subscriptionId: String

        enum CodingKeys: String, CodingKey {
            case requestId = "request_id"
            case subscriptionId = "subscription_id"
        }
    }

    static let pageSize = 50

    let api: HitoAPI

    public init(api: HitoAPI) {
        self.api = api
    }

    public func subscriptions() async throws -> [Subscription] {
        try await api.get(["api", "v1", "podcasts"], as: [Subscription].self)
    }

    public func subscribe(feed: String, request: String) async throws -> String {
        let receipt: Receipt
        do {
            receipt = try await api.post(["api", "v1", "podcasts"], body: Subscribe(requestId: request, feedUrl: feed.trimmingCharacters(in: .whitespacesAndNewlines)), as: Receipt.self)
        } catch HitoAPI.Problem.refused(let status) where status == 400 || status == 422 {
            throw Problem.rejected
        }
        guard receipt.requestId == request else { throw Problem.mismatched }
        return receipt.subscriptionId
    }

    public func episodes(_ subscription: String, after: String?) async throws -> EpisodePage {
        var query = [URLQueryItem(name: "limit", value: String(Self.pageSize))]
        if let after { query.append(URLQueryItem(name: "after", value: after)) }
        let wire = try await api.get(["api", "v1", "podcasts", subscription, "episodes"], query: query, as: Wire.self)
        let tracks = wire.items.map(\.recording)
        let saved = tracks.isEmpty ? [] : try await api.post(["api", "v1", "recordings", "progress"], body: SavedRequest(recordingIds: tracks.map(\.id)), as: [Saved].self)
        let positions = Dictionary(saved.map { ($0.recordingId, $0.positionMs) }, uniquingKeysWith: { first, _ in first })
        return EpisodePage(episodes: tracks.map { Episode(track: $0, positionMs: positions[$0.id] ?? 0) }, next: wire.nextCursor, total: wire.total)
    }

    public func refresh(_ subscription: String, request: String) async throws {
        let receipt = try await api.post(["api", "v1", "podcasts", subscription, "refresh"], body: Command(requestId: request), as: Receipt.self)
        guard receipt.requestId == request else { throw Problem.mismatched }
    }

    public func unsubscribe(_ subscription: String, request: String) async throws {
        let receipt = try await api.post(["api", "v1", "podcasts", subscription, "unsubscribe"], body: Command(requestId: request), as: Receipt.self)
        guard receipt.requestId == request else { throw Problem.mismatched }
    }
}
