import Foundation

public struct SourceAttachment: Sendable {
    public enum Problem: Error, Equatable {
        case notYours, taken, linkNotFound

        public var message: String {
            switch self {
            case .notYours: "Sources can only be added to tracks in your own library."
            case .taken: "This match is already a source for another track, or the track changed. Search again."
            case .linkNotFound: "The service couldn't find a track at that link. Check the link and try again."
            }
        }
    }

    public struct Candidate: Codable, Equatable, Sendable {
        public let installationId: String
        public let installationRevision: Int
        public let providerRecordingId: String

        public init(installationId: String, installationRevision: Int, providerRecordingId: String) {
            self.installationId = installationId
            self.installationRevision = installationRevision
            self.providerRecordingId = providerRecordingId
        }

        enum CodingKeys: String, CodingKey {
            case installationId = "installation_id"
            case installationRevision = "installation_revision"
            case providerRecordingId = "provider_recording_id"
        }
    }

    public struct Review: Equatable, Sendable {
        public let stamp: String
        public let title: String
        public let artist: String?
        public let album: String?
        public let durationMs: Int?
    }

    public struct Comparison: Equatable, Sendable {
        public let text: String
        public let close: Bool

        public init(text: String, close: Bool) {
            self.text = text
            self.close = close
        }
    }

    struct Wire: Decodable {
        struct Release: Decodable { let title: String }

        struct Found: Decodable {
            let title: String
            let artist: String
            let durationMs: Int?
            let release: Release?

            enum CodingKeys: String, CodingKey {
                case title, artist, release
                case durationMs = "duration_ms"
            }
        }

        let reviewStamp: String
        let candidate: Found

        enum CodingKeys: String, CodingKey {
            case candidate
            case reviewStamp = "review_stamp"
        }
    }

    struct Lookup: Encodable {
        let installationId: String
        let installationRevision: Int
        let reference: String

        enum CodingKeys: String, CodingKey {
            case reference
            case installationId = "installation_id"
            case installationRevision = "installation_revision"
        }
    }

    struct Attach: Encodable {
        let candidate: Candidate
        let reviewStamp: String

        enum CodingKeys: String, CodingKey {
            case candidate
            case reviewStamp = "review_stamp"
        }
    }

    let api: HitoAPI

    public init(api: HitoAPI) {
        self.api = api
    }

    public func lookup(_ recording: String, installationId: String, revision: Int, reference: String) async throws -> Candidate {
        let body = Lookup(installationId: installationId, installationRevision: revision, reference: reference.trimmingCharacters(in: .whitespacesAndNewlines))
        do {
            return try await refusing { try await api.post(["api", "v1", "recordings", recording, "sources", "lookup"], body: body, as: Candidate.self) }
        } catch Problem.notYours {
            throw Problem.linkNotFound
        }
    }

    public func review(_ recording: String, _ candidate: Candidate) async throws -> Review {
        let wire = try await refusing { try await api.post(["api", "v1", "recordings", recording, "sources", "review"], body: candidate, as: Wire.self) }
        return Review(stamp: wire.reviewStamp, title: wire.candidate.title, artist: Self.artist(wire.candidate.artist), album: wire.candidate.release?.title, durationMs: wire.candidate.durationMs)
    }

    public func attach(_ recording: String, _ candidate: Candidate, stamp: String) async throws -> Track {
        try await refusing { try await api.post(["api", "v1", "recordings", recording, "sources", "attach"], body: Attach(candidate: candidate, reviewStamp: stamp), as: Track.self) }
    }

    private func refusing<Value>(_ work: () async throws -> Value) async throws -> Value {
        do {
            return try await work()
        } catch HitoAPI.Problem.refused(404) {
            throw Problem.notYours
        } catch HitoAPI.Problem.refused(409) {
            throw Problem.taken
        }
    }

    public static func artist(_ name: String) -> String? {
        let trimmed = name.trimmingCharacters(in: .whitespaces)
        return trimmed.isEmpty || trimmed.lowercased() == "unknown artist" ? nil : trimmed
    }

    public static func compare(saved: Int?, candidate: Int?) -> Comparison? {
        guard let candidate else { return Comparison(text: "The candidate does not give a length.", close: false) }
        guard let saved, saved > 0 else { return nil }
        let difference = candidate - saved
        if abs(difference) <= 3000 { return Comparison(text: "The lengths match.", close: true) }
        let seconds = Int((Double(abs(difference)) / 1000).rounded())
        let amount = abs(difference) < 60_000 ? "\(seconds) seconds" : clock(abs(difference))
        return Comparison(text: "The candidate is \(amount) \(difference > 0 ? "longer" : "shorter") than your saved track.", close: false)
    }

    public static func clock(_ ms: Int) -> String {
        let total = ms / 1000
        return "\(total / 60):\(String(format: "%02d", total % 60))"
    }
}
