import Foundation

public struct SourcePreparation: Sendable {
    public struct Status: Equatable, Sendable {
        public let message: String
        public let percent: Int?
        public let eta: String?
        public let reason: String?
        public let ready: Bool
        public let finished: Bool
    }

    static let phases: [String: String] = [
        "queued": "Audio preparation is queued.",
        "submitting": "Requesting audio preparation…",
        "preparing": "Preparing audio…",
        "ready": "Audio preparation finished.",
        "blocked": "Audio preparation is blocked.",
        "failed": "Audio preparation failed.",
        "submission_unknown": "The music service request could not be confirmed.",
        "cancelled": "Audio preparation stopped because access changed.",
    ]

    static let reasons: [String: String] = [
        "authorization_required": "Reconnect your music service to continue.",
        "quota_reached": "Your music service quota has been reached.",
        "rate_limited": "Your music service is limiting requests.",
        "provider_restricted": "Your music service restricts this audio.",
        "source_unavailable": "This audio is unavailable from your music service.",
        "source_changed": "This audio source changed. Check availability again.",
        "unsupported": "Your music service does not support this preparation.",
        "provider_unavailable": "Your music service is temporarily unavailable.",
        "invalid_response": "Your music service returned an invalid response.",
    ]

    static let settled: Set<String> = ["ready", "blocked", "failed", "submission_unknown", "cancelled"]

    struct Wire: Decodable {
        let phase: String
        let progressBasisPoints: Int?
        let etaSeconds: Int?
        let reason: String?

        enum CodingKeys: String, CodingKey {
            case phase, reason
            case progressBasisPoints = "progress_basis_points"
            case etaSeconds = "eta_seconds"
        }
    }

    struct Start: Encodable {
        let requestId: String

        enum CodingKeys: String, CodingKey { case requestId = "request_id" }
    }

    struct Receipt: Decodable { let preparationId: String

        enum CodingKeys: String, CodingKey { case preparationId = "preparation_id" }
    }

    let api: HitoAPI

    public init(api: HitoAPI) {
        self.api = api
    }

    public func start(_ recording: String, source: String) async throws {
        _ = try await api.post(["api", "v1", "recordings", recording, "sources", source, "preparation"], body: Start(requestId: UUID().uuidString.lowercased()), as: Receipt.self)
    }

    public func status(_ recording: String, source: String) async throws -> Status? {
        let wire: Wire
        do {
            wire = try await api.get(["api", "v1", "recordings", recording, "sources", source, "preparation"], as: Wire.self)
        } catch HitoAPI.Problem.refused(404) {
            return nil
        }
        let preparing = wire.phase == "preparing"
        return Status(
            message: Self.phases[wire.phase] ?? "Audio preparation is in progress.",
            percent: preparing ? wire.progressBasisPoints.map { $0 / 100 } : nil,
            eta: preparing ? wire.etaSeconds.map { "Estimated time remaining: \($0) seconds." } : nil,
            reason: wire.reason.flatMap { Self.reasons[$0] },
            ready: wire.phase == "ready",
            finished: Self.settled.contains(wire.phase)
        )
    }
}
