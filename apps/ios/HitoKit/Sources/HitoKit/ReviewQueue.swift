import Foundation

public struct ReviewCandidate: Decodable, Equatable, Identifiable, Sendable {
    public let providerArtistId: String
    public let name: String
    public let disambiguation: String?
    public let kind: String?
    public let country: String?
    public let corroboratedTitles: Int
    public let inLibrary: Bool
    public let origin: String

    public var id: String { providerArtistId }

    public var label: String { [name, disambiguation].compactMap { $0 }.joined(separator: ", ") }

    public var details: String {
        var parts = [disambiguation, kind, country.flatMap { Locale(identifier: "en").localizedString(forRegionCode: $0) }].compactMap { $0 }
        if origin == "matched_on_other_tracks" {
            parts.append("Matched on \(ReviewItem.count(corroboratedTitles, "other track", "other tracks")) of this credit")
        } else if corroboratedTitles > 0 {
            parts.append("\(ReviewItem.count(corroboratedTitles, "title", "titles")) match your library")
        }
        if inLibrary { parts.append("already in your artists") }
        if origin == "chosen_artist" {
            parts.insert(contentsOf: ["Same artist as \(name)", "you already chose this artist for another credit"], at: 0)
        }
        return parts.joined(separator: " · ")
    }

    enum CodingKeys: String, CodingKey {
        case name, disambiguation, kind, country, origin
        case providerArtistId = "provider_artist_id"
        case corroboratedTitles = "corroborated_titles"
        case inLibrary = "in_library"
    }
}

public struct ReviewItem: Decodable, Equatable, Identifiable, Sendable {
    public let artistId: String
    public let name: String
    public let undecidedTracks: Int
    public let needsSearch: Bool
    public let searchRecordingId: String?
    public let candidates: [ReviewCandidate]

    public var id: String { artistId }

    public var tracks: String { Self.count(undecidedTracks, "track", "tracks") }

    static func count(_ value: Int, _ one: String, _ many: String) -> String {
        "\(value) \(value == 1 ? one : many)"
    }

    enum CodingKeys: String, CodingKey {
        case name, candidates
        case artistId = "artist_id"
        case undecidedTracks = "undecided_tracks"
        case needsSearch = "needs_search"
        case searchRecordingId = "search_recording_id"
    }
}

public struct ReviewQueue: Sendable {
    static let pageSize = 50
    static let confirmSteps = 10

    struct Confirm: Encodable {
        let requestId: String
        let providerArtistId: String
        let after: String?

        enum CodingKeys: String, CodingKey {
            case after
            case requestId = "request_id"
            case providerArtistId = "provider_artist_id"
        }

        func encode(to encoder: Encoder) throws {
            var container = encoder.container(keyedBy: CodingKeys.self)
            try container.encode(requestId, forKey: .requestId)
            try container.encode(providerArtistId, forKey: .providerArtistId)
            try container.encode(after, forKey: .after)
        }
    }

    struct Confirmation: Decodable {
        let nextAfter: String?

        enum CodingKeys: String, CodingKey {
            case nextAfter = "next_after"
        }
    }

    struct Choice: Encodable {
        let providerArtistId: String?

        enum CodingKeys: String, CodingKey {
            case providerArtistId = "provider_artist_id"
        }

        func encode(to encoder: Encoder) throws {
            var container = encoder.container(keyedBy: CodingKeys.self)
            try container.encodeIfPresent(providerArtistId, forKey: .providerArtistId)
        }
    }

    let api: HitoAPI

    public init(api: HitoAPI) {
        self.api = api
    }

    private static let path = ["api", "v1", "artist-matching", "review-queue"]

    public func waiting(offset: Int) async throws -> Page<ReviewItem> {
        try await api.get(Self.path, query: Self.window(offset), as: Page<ReviewItem>.self)
    }

    public func dismissed(offset: Int) async throws -> Page<ReviewItem> {
        try await api.get(Self.path + ["dismissed"], query: Self.window(offset), as: Page<ReviewItem>.self)
    }

    public func confirm(credit: String, artist: String) async throws {
        var after: String?
        for _ in 0..<Self.confirmSteps {
            let receipt = try await api.post(Self.path + [credit, "confirm"], body: Confirm(requestId: UUID().uuidString.lowercased(), providerArtistId: artist, after: after), as: Confirmation.self)
            guard let next = receipt.nextAfter else { return }
            after = next
        }
    }

    public func dismiss(credit: String, artist: String?) async throws {
        _ = try await api.post(Self.path + [credit, "dismiss"], body: Choice(providerArtistId: artist), as: Nothing.self)
    }

    public func restore(credit: String, artist: String?) async throws {
        _ = try await api.post(Self.path + [credit, "restore"], body: Choice(providerArtistId: artist), as: Nothing.self)
    }

    private static func window(_ offset: Int) -> [URLQueryItem] {
        [URLQueryItem(name: "limit", value: String(pageSize)), URLQueryItem(name: "offset", value: String(offset))]
    }
}
