import Foundation

public struct Track: Decodable, Equatable, Hashable, Identifiable, Sendable {
    public struct Credit: Decodable, Equatable, Hashable, Sendable {
        public let id: String
        public let name: String

        public init(id: String, name: String) {
            self.id = id
            self.name = name
        }
    }

    public let id: String
    public let title: String
    public let artist: String
    public let album: String?
    public let releaseId: String?
    public let durationMs: Int
    public let available: Bool
    public let playsFromServices: Bool
    public let liked: Bool
    public var trackNumber: Int?
    public let artistId: String?
    public let credits: [Credit]?

    public init(id: String, title: String, artist: String, album: String?, releaseId: String?, durationMs: Int, available: Bool, playsFromServices: Bool, liked: Bool, trackNumber: Int? = nil, artistId: String? = nil, credits: [Credit]? = nil) {
        self.id = id
        self.title = title
        self.artist = artist
        self.album = album
        self.releaseId = releaseId
        self.durationMs = durationMs
        self.available = available
        self.playsFromServices = playsFromServices
        self.liked = liked
        self.trackNumber = trackNumber
        self.artistId = artistId
        self.credits = credits
    }

    public var isPlayable: Bool { available || playsFromServices }

    static let unlinked = "entry:"

    public var canFind: Bool { !isPlayable && !id.hasPrefix(Self.unlinked) }

    public var sourceAction: String? {
        id.hasPrefix(Self.unlinked) ? nil : isPlayable ? "Fix source" : "Find audio"
    }

    public var artworkPath: [String] { ["api", "v1", "recordings", id, "artwork"] }

    enum CodingKeys: String, CodingKey {
        case id, title, artist, album, available, liked
        case releaseId = "release_id"
        case durationMs = "duration_ms"
        case playsFromServices = "plays_from_services"
        case trackNumber = "track_number"
        case artistId = "artist_id"
        case credits = "credited_artists"
    }
}

public struct Album: Decodable, Equatable, Hashable, Identifiable, Sendable {
    public let id: String
    public let title: String
    public let artist: String?
    public let trackCount: Int
    public let year: Int?
    public let durationMs: Int?
    public let credit: String?

    public init(id: String, title: String, artist: String?, trackCount: Int, year: Int?, durationMs: Int?, credit: String? = nil) {
        self.id = id
        self.title = title
        self.artist = artist
        self.trackCount = trackCount
        self.year = year
        self.durationMs = durationMs
        self.credit = credit ?? Self.own(artist)
    }

    public var artworkPath: [String] { ["api", "v1", "releases", id, "artwork"] }

    enum CodingKeys: String, CodingKey {
        case id, title, artist, year
        case trackCount = "track_count"
        case durationMs = "duration_ms"
        case inferredArtist = "inferred_artist"
    }

    struct Inferred: Decodable {
        let kind: String
        let name: String?
    }

    public init(from decoder: Decoder) throws {
        let values = try decoder.container(keyedBy: CodingKeys.self)
        let artist = try values.decodeIfPresent(String.self, forKey: .artist)
        let inferred = try? values.decodeIfPresent(Inferred.self, forKey: .inferredArtist)
        self.init(
            id: try values.decode(String.self, forKey: .id),
            title: try values.decode(String.self, forKey: .title),
            artist: artist,
            trackCount: try values.decode(Int.self, forKey: .trackCount),
            year: try values.decodeIfPresent(Int.self, forKey: .year),
            durationMs: try values.decodeIfPresent(Int.self, forKey: .durationMs),
            credit: Self.own(artist) ?? Self.credit(inferred)
        )
    }

    static func own(_ artist: String?) -> String? {
        guard let trimmed = artist?.trimmingCharacters(in: .whitespaces), !trimmed.isEmpty else { return nil }
        return trimmed
    }

    static func credit(_ inferred: Inferred?) -> String? {
        switch inferred?.kind {
        case "named": inferred?.name
        case "various": "Various artists"
        default: nil
        }
    }
}

public struct AlbumDetail: Decodable, Equatable, Sendable {
    public let album: Album
    public let tracks: [Track]
    public let savedEdition: SavedEdition?
    public let anchor: CatalogAnchor?
    public let choiceRevision: Int
    public let needsChoice: Bool

    struct Tracks: Decodable { let items: [Track] }

    struct Choice: Decodable {
        struct State: Decodable {
            let status: String
            let edition: SavedEdition?
        }

        let revision: Int
        let state: State
    }

    enum CodingKeys: String, CodingKey {
        case album = "release"
        case tracks
        case savedEdition = "saved_edition"
        case anchor = "catalog_anchor"
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        album = try container.decode(Album.self, forKey: .album)
        tracks = try container.decode(Tracks.self, forKey: .tracks).items
        let choice = try? container.decodeIfPresent(Choice.self, forKey: .savedEdition)
        savedEdition = choice?.state.status == "ready" ? choice?.state.edition : nil
        anchor = try? container.decodeIfPresent(CatalogAnchor.self, forKey: .anchor)
        choiceRevision = choice?.revision ?? 0
        needsChoice = choice?.state.status == "needs_choice"
    }
}

public struct Library: Sendable {
    public static let pageSize = 50
    let api: HitoAPI

    public init(api: HitoAPI) {
        self.api = api
    }

    public func filtersByAvailability() async throws -> Bool {
        let query = [URLQueryItem(name: "offset", value: "0"), URLQueryItem(name: "limit", value: "1")]
        let (_, response) = try await api.getWithResponse(["api", "v1", "recordings"], query: query, as: Page<Track>.self)
        return response.value(forHTTPHeaderField: Availability.header) == "all"
    }

    public func tracks(offset: Int, search: String? = nil, liked: Bool = false, artist: String? = nil, sort: TrackSort? = nil, availability: Availability? = nil) async throws -> Page<Track> {
        var query = paging(offset)
        if let sort { query.append(URLQueryItem(name: "sort", value: sort.rawValue)) }
        if liked { query.append(URLQueryItem(name: "liked", value: "true")) }
        if let artist { query.append(URLQueryItem(name: "artist", value: artist)) }
        if let search, !search.isEmpty { query.append(URLQueryItem(name: "q", value: search)) }
        if let availability { query.append(URLQueryItem(name: "availability", value: availability.rawValue)) }
        do {
            let (page, response) = try await api.getWithResponse(["api", "v1", "recordings"], query: query, as: Page<Track>.self)
            if let availability, response.value(forHTTPHeaderField: Availability.header) != availability.rawValue {
                throw Availability.Unconfirmed()
            }
            return page
        } catch HitoAPI.Problem.refused(400) {
            guard let sort, [.album, .duration].contains(sort) else { throw HitoAPI.Problem.refused(400) }
            throw TrackSort.Unsupported(sort: sort)
        }
    }

    public func albums(offset: Int, search: String? = nil) async throws -> Page<Album> {
        let filter = search.map { [URLQueryItem(name: "q", value: $0)] } ?? [URLQueryItem(name: "sort", value: "recent")]
        return try await api.get(["api", "v1", "releases"], query: paging(offset) + filter, as: Page<Album>.self)
    }

    public func album(_ id: String) async throws -> AlbumDetail {
        try await api.get(["api", "v1", "releases", id], query: [URLQueryItem(name: "offset", value: "0"), URLQueryItem(name: "limit", value: "200")], as: AlbumDetail.self)
    }

    private func paging(_ offset: Int) -> [URLQueryItem] {
        [URLQueryItem(name: "offset", value: String(offset)), URLQueryItem(name: "limit", value: String(Self.pageSize))]
    }
}

public enum Availability: String, CaseIterable, Sendable {
    case playable = "available_or_service"
    case noSource = "no_source"

    static let header = "Hito-Recordings-Availability"

    public struct Unconfirmed: Error, Equatable {
        public init() {}
        public let message = "This server didn't apply the filter. Update the server to use it."
    }

    public var label: String {
        switch self {
        case .playable: "Has a source"
        case .noSource: "No source"
        }
    }
}

public struct Artist: Decodable, Equatable, Hashable, Identifiable, Sendable {
    public let id: String
    public let name: String
    public let trackCount: Int
    public let albumCount: Int
    public let appearanceCount: Int?

    public init(id: String, name: String, trackCount: Int, albumCount: Int, appearanceCount: Int? = nil) {
        self.id = id
        self.name = name
        self.trackCount = trackCount
        self.albumCount = albumCount
        self.appearanceCount = appearanceCount
    }

    public var countLabel: String {
        func count(_ value: Int, _ one: String, _ many: String) -> String { "\(value) \(value == 1 ? one : many)" }
        if trackCount > 0 { return count(trackCount, "track", "tracks") }
        if albumCount > 0 { return count(albumCount, "album", "albums") }
        return count(appearanceCount ?? 0, "appearance", "appearances")
    }

    enum CodingKeys: String, CodingKey {
        case id, name
        case trackCount = "recording_count"
        case albumCount = "release_count"
        case appearanceCount = "featured_recording_count"
    }
}

extension Library {
    public func artists(offset: Int, search: String? = nil) async throws -> Page<Artist> {
        var query = [URLQueryItem(name: "offset", value: String(offset)), URLQueryItem(name: "limit", value: String(Self.pageSize))]
        if let search, !search.isEmpty { query.append(URLQueryItem(name: "q", value: search)) }
        return try await api.get(["api", "v1", "artists"], query: query, as: Page<Artist>.self)
    }
}

public enum TrackSort: String, CaseIterable, Sendable {
    case recent, title, artist, year, album, duration

    public struct Unsupported: Error, Equatable {
        public let sort: TrackSort

        public var message: String { "This server can't sort by \(sort.label.lowercased()) yet. Update the server to use it." }
    }

    public var label: String {
        switch self {
        case .recent: "Recently added"
        case .title: "Title"
        case .artist: "Artist"
        case .year: "Year"
        case .album: "Album"
        case .duration: "Duration"
        }
    }
}
