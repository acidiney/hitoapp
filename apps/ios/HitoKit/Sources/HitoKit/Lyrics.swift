import Foundation

public struct LyricLine: Equatable, Hashable, Sendable {
    public let start: Double
    public let end: Double
    public let text: String
}

public enum LyricsResult: Equatable, Sendable {
    case lines([LyricLine], from: String)
    case none
}

public struct Lyrics: Sendable {
    struct Providers: Decodable { let items: [Provider] }

    struct Provider: Decodable {
        let observationId: String
        let installationRevision: Int
        let label: String

        enum CodingKeys: String, CodingKey {
            case label
            case observationId = "observation_id"
            case installationRevision = "installation_revision"
        }
    }

    struct Read: Encodable {
        let observationId: String
        let installationRevision: Int

        enum CodingKeys: String, CodingKey {
            case observationId = "observation_id"
            case installationRevision = "installation_revision"
        }
    }

    struct Lookup: Decodable {
        let status: String
        let document: Document?
    }

    struct Document: Decodable { let lyrics: Timed? }
    struct Timed: Decodable { let lines: [Line] }

    struct Line: Decodable {
        let startMs: Int
        let endMs: Int
        let text: String

        enum CodingKeys: String, CodingKey {
            case text
            case startMs = "start_ms"
            case endMs = "end_ms"
        }
    }

    let api: HitoAPI

    public init(api: HitoAPI) {
        self.api = api
    }

    public func load(_ recordingId: String) async throws -> LyricsResult {
        let providers = try await api.get(["api", "v1", "recordings", recordingId, "lyrics", "providers"], as: Providers.self)
        guard let provider = providers.items.first else { return .none }
        let lookup = try await api.post(
            ["api", "v1", "recordings", recordingId, "lyrics"],
            body: Read(observationId: provider.observationId, installationRevision: provider.installationRevision),
            as: Lookup.self
        )
        guard lookup.status == "ready", let lines = lookup.document?.lyrics?.lines, !lines.isEmpty else { return .none }
        return .lines(lines.map { LyricLine(start: Double($0.startMs) / 1000, end: Double($0.endMs) / 1000, text: $0.text) }, from: provider.label)
    }
}
