import Foundation

public struct ArtworkRetrieval: Sendable {
    public enum Kind: String, Sendable {
        case recording, release, playlist

        var collection: String { "\(rawValue)s" }

        var receipts: String {
            switch self {
            case .recording: "artwork-fetches"
            case .release: "release-artwork-fetches"
            case .playlist: "playlist-artwork-fetches"
            }
        }

        var owner: String { "\(rawValue)_id" }
    }

    public struct Subject: Hashable, Sendable {
        public let kind: Kind
        public let id: String

        public init(kind: Kind, id: String) {
            self.kind = kind
            self.id = id
        }

        public init?(path: [String]) {
            guard path.count == 5, path[0] == "api", path[1] == "v1", path[4] == "artwork",
                  let kind = [Kind.recording, .release, .playlist].first(where: { $0.collection == path[2] }) else { return nil }
            self.init(kind: kind, id: path[3])
        }
    }

    public enum Outcome: Equatable, Sendable {
        case ready, unavailable, failed, cancelled
    }

    struct Availability: Decodable {
        let canFetch: Bool

        enum CodingKeys: String, CodingKey {
            case canFetch = "can_fetch"
        }
    }

    struct Receipt: Decodable {
        let id: String
        let status: String
        let owner: String?

        init(from decoder: Decoder) throws {
            let container = try decoder.container(keyedBy: Key.self)
            id = try container.decode(String.self, forKey: Key("id"))
            status = try container.decode(String.self, forKey: Key("status"))
            owner = ["recording_id", "release_id", "playlist_id"].lazy.compactMap { try? container.decode(String.self, forKey: Key($0)) }.first
        }

        struct Key: CodingKey {
            let stringValue: String
            var intValue: Int? { nil }
            init(_ value: String) { stringValue = value }
            init?(stringValue: String) { self.stringValue = stringValue }
            init?(intValue: Int) { nil }
        }
    }

    static let admissionAttempts = 5
    static let polls = 120

    let api: HitoAPI
    let pause: @Sendable (Duration) async throws -> Void

    public init(api: HitoAPI, pause: @escaping @Sendable (Duration) async throws -> Void = { try await Task.sleep(for: $0) }) {
        self.api = api
        self.pause = pause
    }

    public func retrieve(_ subject: Subject) async throws -> Outcome {
        let base = ["api", "v1", subject.kind.collection, subject.id, "artwork"]
        let availability = try await api.get(base + ["availability"], as: Availability.self)
        guard availability.canFetch else { return .unavailable }
        var receipt = try await admit(base + ["fetch"])
        for _ in 0..<Self.polls {
            guard receipt.owner == subject.id else { return .failed }
            switch receipt.status {
            case "ready": return .ready
            case "cancelled": return .cancelled
            case "queued", "running": break
            default: return .failed
            }
            try await pause(.seconds(1))
            receipt = try await api.get(["api", "v1", subject.kind.receipts, receipt.id], as: Receipt.self)
        }
        return .failed
    }

    private func admit(_ path: [String]) async throws -> Receipt {
        var rejected = 0
        while true {
            do {
                return try await api.post(path, body: Optional<String>.none, as: Receipt.self)
            } catch HitoAPI.Problem.refused(429) where rejected + 1 < Self.admissionAttempts {
                rejected += 1
                try await pause(.seconds(2))
            }
        }
    }
}
