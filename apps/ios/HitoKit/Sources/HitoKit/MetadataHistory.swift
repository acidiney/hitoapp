import Foundation

public struct MetadataHistory: Sendable {
    public enum Origin: Equatable, Sendable {
        case file
        case provider(recording: String?, revision: String?)
        case legacy
    }

    public struct Tag: Equatable, Sendable {
        public let label: String
        public let value: String?
        public let place: String?
    }

    public struct Observation: Equatable, Identifiable, Sendable {
        public let id: String
        public let heading: String
        public let summary: String
        public let when: Date?
        public let origin: Origin
        public let tags: [Tag]
    }

    public struct Page: Equatable, Sendable {
        public let observations: [Observation]
        public let selected: String?
        public let next: Int?
    }

    struct Location: Decodable {
        let kind: String
        let index: Int?
    }

    struct Observed: Decodable {
        let value: String
        let location: Location
    }

    struct Tags: Decodable {
        let title: Observed?
        let artist: Observed?
        let album: Observed?
        let albumArtist: Observed?
        let disc: Observed?
        let track: Observed?

        enum CodingKeys: String, CodingKey {
            case title, artist, album, disc, track
            case albumArtist = "album_artist"
        }

        var ordered: [(String, Observed?)] {
            [("Title", title), ("Artist", artist), ("Album", album), ("Album artist", albumArtist), ("Disc", disc), ("Track", track)]
        }
    }

    struct Wire: Decodable {
        struct Selected: Decodable { let id: String }

        struct Item: Decodable {
            let id: String
            let provider: String
            let origin: String
            let observedAt: Int?
            let providerRecordingId: String?
            let providerRevision: String?
            let title: String
            let artist: String
            let album: String?
            let tags: Tags?

            enum CodingKeys: String, CodingKey {
                case id, provider, origin, title, artist, album, tags
                case observedAt = "observed_at"
                case providerRecordingId = "provider_recording_id"
                case providerRevision = "provider_revision"
            }
        }

        struct Observations: Decodable {
            let items: [Item]
            let nextBefore: Int?

            enum CodingKeys: String, CodingKey {
                case items
                case nextBefore = "next_before"
            }
        }

        let selectedObservation: Selected?
        let observations: Observations

        enum CodingKeys: String, CodingKey {
            case observations
            case selectedObservation = "selected_observation"
        }
    }

    let api: HitoAPI

    public init(api: HitoAPI) {
        self.api = api
    }

    public func page(_ recording: String, before: Int?) async throws -> Page {
        let query = before.map { [URLQueryItem(name: "before", value: String($0))] } ?? []
        let wire = try await api.get(["api", "v1", "recordings", recording, "metadata"], query: query, as: Wire.self)
        return Page(observations: wire.observations.items.map(Self.observation), selected: wire.selectedObservation?.id, next: wire.observations.nextBefore)
    }

    static func observation(_ item: Wire.Item) -> Observation {
        let origin: Origin = switch item.origin {
        case "provider_response": .provider(recording: item.providerRecordingId, revision: item.providerRevision)
        case "legacy_normalized": .legacy
        default: .file
        }
        let tags: [Tag] = origin == .file ? (item.tags ?? Tags(title: nil, artist: nil, album: nil, albumArtist: nil, disc: nil, track: nil)).ordered.map { label, tag in
            let place: String? = tag.map { $0.location.kind == "format" ? "File tags" : "Audio stream \(($0.location.index ?? 0) + 1)" }
            return Tag(label: label, value: tag?.value, place: place)
        } : []
        return Observation(
            id: item.id,
            heading: "Metadata read from \(item.provider == "local" ? "local file" : item.provider)",
            summary: "\(item.title) · \(item.artist) · \(item.album ?? "Unknown album")",
            when: item.observedAt.map { Date(timeIntervalSince1970: TimeInterval($0)) },
            origin: origin,
            tags: tags
        )
    }
}
