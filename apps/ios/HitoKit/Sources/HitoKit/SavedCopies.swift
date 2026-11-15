import Foundation

public struct SavedCopy: Equatable, Sendable {
    public enum State: String, Decodable, Sendable {
        case queued, running, verifying, ready, failed
    }

    public let intentId: String
    public let state: State
    public let error: String?
    public let bytesReceived: Int
    public let totalBytes: Int?
    public let matchesSourceChoice: Bool

    public init(intentId: String, state: State, error: String?, bytesReceived: Int, totalBytes: Int?, matchesSourceChoice: Bool) {
        self.intentId = intentId
        self.state = state
        self.error = error
        self.bytesReceived = bytesReceived
        self.totalBytes = totalBytes
        self.matchesSourceChoice = matchesSourceChoice
    }

    public var transferring: Bool { [.queued, .running, .verifying].contains(state) }

    public var progress: Double? {
        guard state == .running, let totalBytes, totalBytes > 0 else { return nil }
        return min(1, Double(bytesReceived) / Double(totalBytes))
    }

    public var summary: String {
        switch state {
        case .ready:
            return matchesSourceChoice ? "Saved on your server. Plays from the copy when the service can't." : "A copy from a previous source is saved on your server."
        case .queued where error == "insufficient_storage":
            return "Not enough disk space on your server. Hito will try again."
        case .queued where error == "provider_unavailable":
            return "The music service didn't deliver the audio. Hito will try again."
        case .queued:
            return "Waiting to save a copy."
        case .running:
            guard let totalBytes, totalBytes > 0 else { return "Saving a copy on your server…" }
            return "Saving a copy on your server… \(megabytes(bytesReceived)) of \(megabytes(totalBytes)) MB"
        case .verifying:
            return "Checking the saved audio…"
        case .failed:
            return "The copy wasn't saved. Remove it, then try again."
        }
    }

    private func megabytes(_ bytes: Int) -> String {
        String(format: "%.1f", Double(bytes) / 1_000_000)
    }
}

public struct SavedCopies: Sendable {
    public struct Refused: Error, Equatable {
        public let reason: String
    }

    struct Status: Decodable {
        let status: SavedCopy.State
        let intentId: String
        let error: String?
        let bytesReceived: Int
        let totalBytes: Int?
        let matchesSourceChoice: Bool

        enum CodingKeys: String, CodingKey {
            case status, error
            case intentId = "intent_id"
            case bytesReceived = "bytes_received"
            case totalBytes = "total_bytes"
            case matchesSourceChoice = "matches_source_choice"
        }
    }

    struct Request: Encodable {
        let requestId: UUID

        enum CodingKeys: String, CodingKey { case requestId = "request_id" }
    }

    struct Receipt: Decodable {
        let intentId: String

        enum CodingKeys: String, CodingKey { case intentId = "intent_id" }
    }

    let api: HitoAPI

    public init(api: HitoAPI) {
        self.api = api
    }

    public func status(_ recording: String) async throws -> SavedCopy? {
        let found: Status?
        do {
            found = try await api.get(["api", "v1", "recordings", recording, "download"], as: Status?.self)
        } catch HitoAPI.Problem.refused(404) {
            return nil
        }
        return found.map { SavedCopy(intentId: $0.intentId, state: $0.status, error: $0.error, bytesReceived: $0.bytesReceived, totalBytes: $0.totalBytes, matchesSourceChoice: $0.matchesSourceChoice) }
    }

    public func save(_ recording: String, request: UUID) async throws -> String {
        do {
            return try await api.postExplained(["api", "v1", "recordings", recording, "download"], body: Request(requestId: request), as: Receipt.self).intentId
        } catch let refusal as HitoAPI.Refusal {
            guard refusal.status == 409 else { throw HitoAPI.Problem.refused(refusal.status) }
            throw Refused(reason: refusal.reason ?? "Hito can't save a copy of this track right now.")
        }
    }

    public func remove(_ intent: String) async throws {
        _ = try await api.delete(["api", "v1", "downloads", intent], as: Nothing.self)
    }
}
