import Foundation

public struct AudioRequest: Equatable, Sendable {
    public let leaseId: String
    public let recordingId: String
    public let sourceId: String
    public let sourceLabel: String
    public let url: URL
    public let headers: [String: String]
}

public struct PlaybackSource: Equatable, Identifiable, Sendable {
    public let id: String
    public let label: String
    public let provider: String
    public let ready: Bool
    public let bestGuess: Bool

    public var missingFile: Bool { provider == "local" && !ready }
}

public struct Playback: Sendable {
    public enum Problem: Error, Equatable {
        case noReadySource
    }

    struct Source: Decodable {
        let id: String
        let label: String
        let provider: String
        let providerLabel: String?
        let ready: Bool
        let matchKind: String

        enum CodingKeys: String, CodingKey {
            case id, label, ready, provider
            case providerLabel = "provider_label"
            case matchKind = "match_kind"
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
        var seen: [String: Int] = [:]
        return try await listed(recordingId).map { source in
            let name = source.providerLabel ?? source.label
            let count = seen[source.provider, default: 0] + 1
            seen[source.provider] = count
            let ordinals = [2: "second", 3: "third", 4: "fourth", 5: "fifth"]
            let ordinal = ordinals[count].map { "\($0) match" } ?? "match \(count)"
            let label = count == 1 ? name : "\(name), \(ordinal)"
            return PlaybackSource(id: source.id, label: label, provider: source.provider, ready: source.ready, bestGuess: source.matchKind == "best_guess")
        }
    }

    public func prepare(_ recordingId: String, source chosen: String) async throws -> AudioRequest {
        let others = try await listed(recordingId).map(\.id).filter { $0 != chosen }
        return try await prepare(recordingId, skipping: Set(others))
    }

    public func release(_ leaseId: String) async {
        _ = try? await api.delete(["api", "v1", "playback", "leases", leaseId], as: Nothing.self)
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
                leaseId: lease.id,
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
