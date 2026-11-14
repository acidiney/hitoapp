import Foundation

public struct AudioRequest: Equatable, Sendable {
    public let recordingId: String
    public let sourceId: String
    public let sourceLabel: String
    public let url: URL
    public let headers: [String: String]
}

public struct PlaybackSource: Equatable, Identifiable, Sendable {
    public let id: String
    public let label: String
    public let ready: Bool
}

public struct Playback: Sendable {
    public enum Problem: Error, Equatable {
        case noReadySource
    }

    struct Source: Decodable {
        let id: String
        let label: String
        let providerLabel: String?
        let ready: Bool

        enum CodingKeys: String, CodingKey {
            case id, label, ready
            case providerLabel = "provider_label"
        }
    }

    struct Begin: Encodable {
        let recordingId: String
        let sourceId: String

        enum CodingKeys: String, CodingKey {
            case recordingId = "recording_id"
            case sourceId = "source_id"
        }
    }

    struct Lease: Decodable { let id: String }

    let api: HitoAPI

    public init(api: HitoAPI) {
        self.api = api
    }

    public func sources(_ recordingId: String) async throws -> [PlaybackSource] {
        try await listed(recordingId).map { PlaybackSource(id: $0.id, label: $0.providerLabel ?? $0.label, ready: $0.ready) }
    }

    public func prepare(_ recordingId: String, source chosen: String) async throws -> AudioRequest {
        let others = try await listed(recordingId).map(\.id).filter { $0 != chosen }
        return try await prepare(recordingId, skipping: Set(others))
    }

    private func listed(_ recordingId: String) async throws -> [Source] {
        try await api.get(["api", "v1", "recordings", recordingId, "playback-sources"], as: [Source].self)
    }

    public func prepare(_ recordingId: String, skipping tried: Set<String> = []) async throws -> AudioRequest {
        let sources = try await listed(recordingId)
        for source in sources where source.ready && !tried.contains(source.id) {
            try Task.checkCancellation()
            let lease: Lease
            do {
                lease = try await api.post(["api", "v1", "playback", "leases"], body: Begin(recordingId: recordingId, sourceId: source.id), as: Lease.self)
            } catch HitoAPI.Problem.refused {
                continue
            }
            return AudioRequest(
                recordingId: recordingId,
                sourceId: source.id,
                sourceLabel: source.providerLabel ?? source.label,
                url: api.address.url(["api", "v1", "playback", "leases", lease.id, "audio"]),
                headers: api.authorization
            )
        }
        throw Problem.noReadySource
    }
}
