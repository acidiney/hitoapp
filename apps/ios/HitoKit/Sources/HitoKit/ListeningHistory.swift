import Foundation

public struct ListeningHistory: Sendable {
    public struct Entry: Equatable, Identifiable, Sendable {
        public let track: Track
        public let positionMs: Int

        public var id: String { track.id }
    }

    public struct Found: Equatable, Sendable {
        public let entries: [Entry]
        public let enabled: Bool

        public var summary: String { "\(entries.count) recently played \(entries.count == 1 ? "track" : "tracks")" }
    }

    struct Row: Decodable {
        let track: Track
        let positionMs: Int

        enum CodingKeys: String, CodingKey { case positionMs = "position_ms" }

        init(from decoder: Decoder) throws {
            track = try Track(from: decoder)
            positionMs = try decoder.container(keyedBy: CodingKeys.self).decode(Int.self, forKey: .positionMs)
        }
    }

    struct Activity: Decodable { let history: [Row] }
    struct Preference: Decodable { let enabled: Bool }

    let api: HitoAPI

    public init(api: HitoAPI) {
        self.api = api
    }

    public func load() async throws -> Found {
        async let activity = api.get(["api", "v1", "activity"], as: Activity.self)
        async let preference = api.get(["api", "v1", "history", "preferences"], as: Preference.self)
        let (rows, saved) = try await (activity, preference)
        return Found(entries: rows.history.map { Entry(track: $0.track, positionMs: $0.positionMs) }, enabled: saved.enabled)
    }
}
