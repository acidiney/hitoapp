import Foundation

public struct PlaylistFollowing: Sendable {
    public enum Problem: Error, Equatable {
        case busy
    }

    public enum State: String, Decodable, Sendable {
        case ready, queued, running, failed, paused

        public var label: String {
            switch self {
            case .ready: "Ready"
            case .queued: "Queued"
            case .running: "Updating"
            case .failed: "Update failed"
            case .paused: "Paused"
            }
        }
    }

    public struct Change: Decodable, Equatable, Sendable {
        public let kind: String
        public let before: [String]
        public let after: [String]

        public var label: String {
            switch kind {
            case "capabilities": "Abilities"
            case "credential": "Access token"
            default: "Connection"
            }
        }

        public static func value(_ values: [String]) -> String {
            values.isEmpty ? "None" : values.map { $0.replacingOccurrences(of: "_", with: " ") }.sorted().joined(separator: ", ")
        }
    }

    public struct Review: Decodable, Equatable, Sendable {
        public let connectionName: String
        public let installationRevision: Int
        public let serviceRevision: Int
        public let resumable: Bool
        public let changes: [Change]

        enum CodingKeys: String, CodingKey {
            case resumable, changes
            case connectionName = "connection_name"
            case installationRevision = "installation_revision"
            case serviceRevision = "service_revision"
        }

        static let gainedOrder: [(String, String)] = [
            ("search", "song search"),
            ("sources", "playback"),
            ("source_links", "song lookup from pasted links"),
            ("playlists", "playlist import"),
            ("catalog", "album and artist browsing"),
            ("metadata", "track details"),
            ("lyrics", "lyrics"),
            ("artwork", "album covers"),
            ("source_preparation", "faster playback"),
            ("backing_tracks", "backing tracks"),
            ("featured", "featured music on Discover"),
            ("dashboard", "a service dashboard"),
        ]

        static func joined(_ items: [String]) -> String {
            items.count < 2 ? items.joined() : "\(items.dropLast().joined(separator: ", ")) and \(items[items.count - 1])"
        }

        public var sentence: String {
            let generic = "The \(connectionName) connection changed."
            guard changes.count == 1, let change = changes.first else { return generic }
            switch change.kind {
            case "removed": return "The \(connectionName) connection was removed or replaced."
            case "credential": return "The saved access token for \(connectionName) changed."
            default: break
            }
            guard change.after.contains("playlists") else { return "The \(connectionName) connection no longer offers playlists." }
            let gained = Self.gainedOrder.filter { change.after.contains($0.0) && !change.before.contains($0.0) }.map(\.1)
            return gained.isEmpty ? generic : "The \(connectionName) connection now also offers \(Self.joined(gained))."
        }
    }

    public struct Follow: Decodable, Equatable, Sendable {
        public let generation: Int
        public let contentRevision: Int
        public let interval: String
        public let state: State
        public let review: Review?
        let lastRefreshedAt: Int
        let nextRefreshAt: Int

        public var busy: Bool { state == .queued || state == .running }
        public var intervalLabel: String { interval == "hourly" ? "Every hour" : "Every day" }
        public var lastChecked: Date { Date(timeIntervalSince1970: TimeInterval(lastRefreshedAt)) }
        public var nextCheck: Date { Date(timeIntervalSince1970: TimeInterval(nextRefreshAt)) }

        enum CodingKeys: String, CodingKey {
            case generation, interval, review
            case state = "status"
            case contentRevision = "content_revision"
            case lastRefreshedAt = "last_refreshed_at"
            case nextRefreshAt = "next_refresh_at"
        }
    }

    struct Refresh: Encodable {
        let requestId: String

        enum CodingKeys: String, CodingKey { case requestId = "request_id" }
    }

    struct Resume: Encodable {
        let kind = "playlist_follow_resume"
        let playlistId: String
        let expectedGeneration: Int
        let installationRevision: Int
        let serviceRevision: Int

        enum CodingKeys: String, CodingKey {
            case kind
            case playlistId = "playlist_id"
            case expectedGeneration = "expected_generation"
            case installationRevision = "installation_revision"
            case serviceRevision = "service_revision"
        }
    }

    struct ResumeChange: Encodable {
        let id: String
        let action: Resume
    }

    struct ResumeBatch: Encodable {
        let deviceId: String
        let operations: [ResumeChange]

        enum CodingKeys: String, CodingKey {
            case operations
            case deviceId = "device_id"
        }
    }

    struct Accepted: Decodable {}

    let api: HitoAPI

    public init(api: HitoAPI) {
        self.api = api
    }

    public func status(_ playlist: String) async throws -> Follow {
        try await api.get(["api", "v1", "playlists", playlist, "follow"], as: Follow.self)
    }

    public func refresh(_ playlist: String, request: String) async throws {
        do {
            _ = try await api.post(["api", "v1", "playlists", playlist, "follow", "refresh"], body: Refresh(requestId: request), as: Accepted.self)
        } catch HitoAPI.Problem.refused(409) {
            throw Problem.busy
        }
    }

    public func resume(_ playlist: String, _ follow: Follow, device: String) async throws {
        guard let review = follow.review else { return }
        let change = ResumeChange(id: UUID().uuidString.lowercased(), action: Resume(playlistId: playlist, expectedGeneration: follow.generation, installationRevision: review.installationRevision, serviceRevision: review.serviceRevision))
        let receipt = try await api.post(["api", "v1", "sync"], body: ResumeBatch(deviceId: device, operations: [change]), as: Likes.Receipt.self)
        guard receipt.acknowledged.map({ $0.lowercased() }).contains(change.id) else { throw Likes.Problem.notAcknowledged }
    }
}
