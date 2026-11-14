import Foundation

public struct Track: Decodable, Equatable, Hashable, Identifiable, Sendable {
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

    public init(id: String, title: String, artist: String, album: String?, releaseId: String?, durationMs: Int, available: Bool, playsFromServices: Bool, liked: Bool, trackNumber: Int? = nil) {
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
    }

    public var isPlayable: Bool { available || playsFromServices }

    public var artworkPath: [String] { ["api", "v1", "recordings", id, "artwork"] }

    enum CodingKeys: String, CodingKey {
        case id, title, artist, album, available, liked
        case releaseId = "release_id"
        case durationMs = "duration_ms"
        case playsFromServices = "plays_from_services"
        case trackNumber = "track_number"
    }
}

public struct Album: Decodable, Equatable, Hashable, Identifiable, Sendable {
    public let id: String
    public let title: String
    public let artist: String?
    public let trackCount: Int
    public let year: Int?
    public let durationMs: Int?

    public init(id: String, title: String, artist: String?, trackCount: Int, year: Int?, durationMs: Int?) {
        self.id = id
        self.title = title
        self.artist = artist
        self.trackCount = trackCount
        self.year = year
        self.durationMs = durationMs
    }

    public var artworkPath: [String] { ["api", "v1", "releases", id, "artwork"] }

    enum CodingKeys: String, CodingKey {
        case id, title, artist, year
        case trackCount = "track_count"
        case durationMs = "duration_ms"
    }
}

public struct AlbumDetail: Decodable, Equatable, Sendable {
    public let album: Album
    public let tracks: [Track]

    struct Tracks: Decodable { let items: [Track] }

    enum CodingKeys: String, CodingKey {
        case album = "release"
        case tracks
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        album = try container.decode(Album.self, forKey: .album)
        tracks = try container.decode(Tracks.self, forKey: .tracks).items
    }
}

public struct Library: Sendable {
    public static let pageSize = 50
    let api: HitoAPI

    public init(api: HitoAPI) {
        self.api = api
    }

    public func tracks(offset: Int, search: String? = nil, liked: Bool = false, artist: String? = nil, sort: TrackSort? = nil) async throws -> Page<Track> {
        var query = paging(offset)
        if let sort { query.append(URLQueryItem(name: "sort", value: sort.rawValue)) }
        if liked { query.append(URLQueryItem(name: "liked", value: "true")) }
        if let artist { query.append(URLQueryItem(name: "artist", value: artist)) }
        if let search, !search.isEmpty { query.append(URLQueryItem(name: "q", value: search)) }
        return try await api.get(["api", "v1", "recordings"], query: query, as: Page<Track>.self)
    }

    public func albums(offset: Int) async throws -> Page<Album> {
        try await api.get(["api", "v1", "releases"], query: paging(offset) + [URLQueryItem(name: "sort", value: "recent")], as: Page<Album>.self)
    }

    public func album(_ id: String) async throws -> AlbumDetail {
        try await api.get(["api", "v1", "releases", id], query: [URLQueryItem(name: "offset", value: "0"), URLQueryItem(name: "limit", value: "200")], as: AlbumDetail.self)
    }

    private func paging(_ offset: Int) -> [URLQueryItem] {
        [URLQueryItem(name: "offset", value: String(offset)), URLQueryItem(name: "limit", value: String(Self.pageSize))]
    }
}

public struct Artist: Decodable, Equatable, Hashable, Identifiable, Sendable {
    public let id: String
    public let name: String
    public let trackCount: Int
    public let albumCount: Int

    public init(id: String, name: String, trackCount: Int, albumCount: Int) {
        self.id = id
        self.name = name
        self.trackCount = trackCount
        self.albumCount = albumCount
    }

    enum CodingKeys: String, CodingKey {
        case id, name
        case trackCount = "recording_count"
        case albumCount = "release_count"
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
    case recent, title, artist, year

    public var label: String {
        switch self {
        case .recent: "Recently added"
        case .title: "Title"
        case .artist: "Artist"
        case .year: "Year"
        }
    }
}
