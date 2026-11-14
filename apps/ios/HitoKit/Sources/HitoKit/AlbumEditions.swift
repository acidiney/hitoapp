import Foundation

public struct SavedEdition: Decodable, Equatable, Sendable {
    public let recordingId: String
    public let associationId: String
    public let releaseGroupId: String
    public let editionId: String

    public init(recordingId: String, associationId: String, releaseGroupId: String, editionId: String) {
        self.recordingId = recordingId
        self.associationId = associationId
        self.releaseGroupId = releaseGroupId
        self.editionId = editionId
    }

    enum CodingKeys: String, CodingKey {
        case recordingId = "recording_id"
        case associationId = "association_id"
        case releaseGroupId = "release_group_id"
        case editionId = "edition_id"
    }
}

public struct EditionTrack: Equatable, Identifiable, Sendable {
    public let id: String
    public let number: String?
    public let title: String
    public let durationMs: Int?
    public let providerRecording: String?
    public let owned: Track?
    public let attached: Bool

    public var note: String? {
        if owned != nil { return nil }
        if attached { return "Already attached to another recording" }
        return "Not in your library"
    }
}

public struct CompleteAlbum: Equatable, Sendable {
    public struct Adoption: Equatable, Sendable {
        public let installation: String
        public let revision: Int
    }

    public let editionName: String
    public let editionId: String
    public let associationId: String
    public let releaseGroupId: String
    public let tracks: [EditionTrack]
    public let adoption: Adoption?

    public var ownedCount: Int { tracks.filter { $0.owned != nil }.count }

    public var summary: String { "\(ownedCount) of \(tracks.count) in your library" }

    public var addable: [EditionTrack] {
        tracks.filter { $0.owned == nil && !$0.attached && (adoption == nil || $0.providerRecording != nil) }
    }

    public func replacing(_ id: String, with recording: Track) -> CompleteAlbum {
        CompleteAlbum(editionName: editionName, editionId: editionId, associationId: associationId, releaseGroupId: releaseGroupId, tracks: tracks.map {
            $0.id == id ? EditionTrack(id: $0.id, number: $0.number, title: $0.title, durationMs: $0.durationMs, providerRecording: $0.providerRecording, owned: recording, attached: false) : $0
        }, adoption: adoption)
    }
}

public enum EditionRead: Equatable, Sendable {
    case available(CompleteAlbum)
    case unavailable(String)
}

public enum TrackAudio: Equatable, Sendable {
    case saved(Track)
    case missing(String)
}

public struct AlbumEditions: Sendable {
    struct Occurrence: Decodable {
        let id: String
        let number: String?
        let kind: String
        let title: String
        let durationMs: Int?
        let recordingId: String?

        enum CodingKeys: String, CodingKey {
            case id, number, kind, title
            case durationMs = "duration_ms"
            case recordingId = "recording_id"
        }
    }

    struct Medium: Decodable { let tracks: [Occurrence] }

    struct Edition: Decodable {
        let id: String
        let releaseGroupId: String
        let title: String
        let media: [Medium]

        enum CodingKeys: String, CodingKey {
            case id, title, media
            case releaseGroupId = "release_group_id"
        }
    }

    struct Match: Decodable {
        let occurrenceId: String
        let recording: Track

        enum CodingKeys: String, CodingKey {
            case recording
            case occurrenceId = "occurrence_id"
        }
    }

    struct Adoption: Decodable {
        let installationId: String
        let installationRevision: Int

        enum CodingKeys: String, CodingKey {
            case installationId = "installation_id"
            case installationRevision = "installation_revision"
        }
    }

    struct Listing: Decodable {
        let status: String
        let associationId: String?
        let edition: Edition?
        let matches: [Match]?
        let attachedOccurrences: [String]?
        let adoption: Adoption?

        enum CodingKeys: String, CodingKey {
            case status, edition, matches, adoption
            case associationId = "association_id"
            case attachedOccurrences = "attached_occurrences"
        }
    }

    struct Found: Decodable {
        let status: String
        let recording: Track?
    }

    struct Find: Encodable {
        let recordingId: String
        let associationId: String
        let releaseGroupId: String
        let editionId: String
        let occurrenceId: String

        enum CodingKeys: String, CodingKey {
            case recordingId = "recording_id"
            case associationId = "association_id"
            case releaseGroupId = "release_group_id"
            case editionId = "edition_id"
            case occurrenceId = "occurrence_id"
        }
    }

    let api: HitoAPI

    public init(api: HitoAPI) {
        self.api = api
    }

    public func read(_ release: String) async throws -> EditionRead {
        let listing = try await api.post(["api", "v1", "albums", release, "edition", "track-list"], body: Optional<String>.none, as: Listing.self)
        guard listing.status == "available", let edition = listing.edition, let associationId = listing.associationId else {
            return .unavailable(Self.explain(listing.status))
        }
        let owned = Dictionary((listing.matches ?? []).map { ($0.occurrenceId, $0.recording) }, uniquingKeysWith: { first, _ in first })
        let attached = Set(listing.attachedOccurrences ?? [])
        let tracks = edition.media.flatMap(\.tracks).filter { $0.kind == "audio" }.map {
            EditionTrack(id: $0.id, number: $0.number, title: $0.title, durationMs: $0.durationMs, providerRecording: $0.recordingId, owned: owned[$0.id], attached: attached.contains($0.id) && owned[$0.id] == nil)
        }
        return .available(CompleteAlbum(
            editionName: edition.title,
            editionId: edition.id,
            associationId: associationId,
            releaseGroupId: edition.releaseGroupId,
            tracks: tracks,
            adoption: listing.adoption.map { CompleteAlbum.Adoption(installation: $0.installationId, revision: $0.installationRevision) }
        ))
    }

    public func add(_ track: EditionTrack, of album: CompleteAlbum, release: String, saved: SavedEdition) async throws -> TrackAudio {
        if let adoption = album.adoption, let provider = track.providerRecording {
            return .saved(try await Plugins(api: api).adopt(adoption.installation, revision: adoption.revision, result: provider))
        }
        let found = try await api.post(["api", "v1", "albums", release, "edition-track-audio"], body: Find(recordingId: saved.recordingId, associationId: album.associationId, releaseGroupId: album.releaseGroupId, editionId: album.editionId, occurrenceId: track.id), as: Found.self)
        if found.status == "saved", let recording = found.recording { return .saved(recording) }
        return .missing(Self.explainFind(found.status))
    }

    static func explain(_ status: String) -> String {
        switch status {
        case "disabled": "Track details from plugins are off in your settings."
        case "unsupported": "The plugin can't list this edition's tracks."
        case "needs_choice": "The saved edition can't be used any more. Choose the edition again on the web."
        case "cancelled": "Reading the edition was cancelled. Try again."
        default: "This edition's track list isn't available. Try again."
        }
    }

    static func explainFind(_ status: String) -> String {
        switch status {
        case "no_match", "insufficient_evidence": "No audio found"
        case "no_providers": "No connected plugin can find audio"
        case "search_off": "Plugin search is off in your settings"
        case "busy": "Hito is busy. Try again."
        case "changed": "The album changed. Reload it."
        case "disabled": "Track details from plugins are off in your settings"
        default: "Couldn't find audio"
        }
    }
}
