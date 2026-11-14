import Foundation

public struct UnheardAlbum: Equatable, Identifiable, Sendable {
    public let id: String
    public let title: String
    public let artist: String?
    public let played: Int
    public let remaining: Int
    public let tracks: [Track]

    public var album: Album { Album(id: id, title: title, artist: artist, trackCount: played + remaining, year: nil, durationMs: nil) }

    public var headline: String { "\(remaining) more \(remaining == 1 ? "track" : "tracks") from \(title)" }

    public var reason: String {
        "You played \(played) \(played == 1 ? "track" : "tracks") from this album. These are the ones you haven't heard yet."
    }
}

public enum QuickTile: Equatable, Identifiable, Sendable {
    case album(Album)
    case liked(Int)
    case playlist(Playlist)

    public var id: String {
        switch self {
        case let .album(album): "album-\(album.id)"
        case .liked: "liked"
        case let .playlist(playlist): "playlist-\(playlist.id)"
        }
    }

    public var title: String {
        switch self {
        case let .album(album): album.title
        case .liked: "Liked tracks"
        case let .playlist(playlist): playlist.name
        }
    }
}

public struct HomeSummary: Equatable, Sendable {
    public let likedCount: Int
    public let recentAlbums: [Album]
    public let unheard: [UnheardAlbum]

    public func tiles(playlists: [Playlist]) -> [QuickTile] {
        var tiles = recentAlbums.prefix(3).map(QuickTile.album)
        if likedCount > 0 { tiles.append(.liked(likedCount)) }
        tiles += playlists.map(QuickTile.playlist)
        tiles += recentAlbums.dropFirst(3).map(QuickTile.album)
        return Array(tiles.prefix(6))
    }
}

public struct HomeFeed: Sendable {
    struct Recent: Decodable {
        let id: String
        let title: String
        let artist: String?
    }

    struct Unheard: Decodable {
        let releaseId: String
        let title: String
        let artist: String?
        let played: Int
        let remaining: Int
        let tracks: [Track]

        enum CodingKeys: String, CodingKey {
            case title, artist, played, remaining, tracks
            case releaseId = "release_id"
        }
    }

    struct Body: Decodable {
        let likedCount: Int
        let recentAlbums: [Recent]
        let unheard: [Unheard]

        enum CodingKeys: String, CodingKey {
            case unheard
            case likedCount = "liked_count"
            case recentAlbums = "recent_albums"
        }
    }

    let api: HitoAPI

    public init(api: HitoAPI) {
        self.api = api
    }

    public func load() async throws -> HomeSummary? {
        let body: Body
        do {
            body = try await api.get(["api", "v1", "home"], as: Body.self)
        } catch HitoAPI.Problem.refused(404) {
            return nil
        }
        return HomeSummary(
            likedCount: body.likedCount,
            recentAlbums: body.recentAlbums.map { Album(id: $0.id, title: $0.title, artist: $0.artist, trackCount: 0, year: nil, durationMs: nil) },
            unheard: body.unheard.map { UnheardAlbum(id: $0.releaseId, title: $0.title, artist: $0.artist, played: $0.played, remaining: $0.remaining, tracks: $0.tracks) }
        )
    }
}
