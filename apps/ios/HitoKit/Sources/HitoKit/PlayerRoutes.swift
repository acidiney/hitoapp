import Foundation

public enum PlayerRoute: Hashable, Sendable {
    case artist(Artist)
    case artistGroup(ArtistGroup)
    case album(Album)
    case playlist(Playlist)

    public var title: String {
        switch self {
        case let .artist(artist): artist.name
        case let .artistGroup(group): group.name
        case let .album(album): album.title
        case let .playlist(playlist): playlist.name
        }
    }

    public var kind: String {
        switch self {
        case .artist: "artist"
        case .artistGroup: "artist_group"
        case .album: "album"
        case .playlist: "playlist"
        }
    }
}

public struct PlayerRoutes: Equatable, Sendable {
    public let artists: [PlayerRoute]
    public let album: PlayerRoute?
    public let playingFrom: PlayerRoute?
    public let needsIdentity: Bool

    public init(track: Track, playlist: Playlist? = nil, identity: ArtistGroup? = nil) {
        var seen = Set<String>()
        let credited = (track.credits ?? []).filter { seen.insert($0.id).inserted }
        let library: [PlayerRoute] = if !credited.isEmpty {
            credited.map { .artist(Artist(id: $0.id, name: $0.name, trackCount: 0, albumCount: 0)) }
        } else if let id = track.artistId {
            [.artist(Artist(id: id, name: track.artist, trackCount: 0, albumCount: 0))]
        } else {
            []
        }
        artists = library.isEmpty ? identity.map { [.artistGroup($0)] } ?? [] : library
        needsIdentity = artists.isEmpty && !track.id.hasPrefix(Track.unlinked)
        album = track.releaseId.map { .album(Album(id: $0, title: track.album ?? track.title, artist: track.artist, trackCount: 0, year: nil, durationMs: nil)) }
        playingFrom = playlist.map(PlayerRoute.playlist) ?? album
    }
}

public struct ArtistIdentity: Sendable {
    struct Choice: Decodable {
        struct State: Decodable {
            let status: String
            let groupId: String?
            let associationId: String?

            enum CodingKeys: String, CodingKey {
                case status
                case groupId = "group_id"
                case associationId = "association_id"
            }
        }

        let state: State
    }

    let api: HitoAPI

    public init(api: HitoAPI) {
        self.api = api
    }

    public func group(for track: Track) async throws -> ArtistGroup? {
        let choice = try await api.get(["api", "v1", "recordings", track.id, "artist-identity"], as: Choice.self)
        guard choice.state.status == "selected", let group = choice.state.groupId, let association = choice.state.associationId else { return nil }
        return ArtistGroup(id: group, name: track.artist, recordingId: track.id, associationId: association, recordingCount: 0, releaseCount: 0)
    }
}
