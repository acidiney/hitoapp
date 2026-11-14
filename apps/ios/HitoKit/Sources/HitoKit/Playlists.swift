import Foundation

public struct Playlist: Decodable, Equatable, Hashable, Identifiable, Sendable {
    public enum Kind: String, Decodable, Sendable {
        case local, followed
    }

    public let id: String
    public let name: String
    public let kind: Kind

    public init(id: String, name: String, kind: Kind) {
        self.id = id
        self.name = name
        self.kind = kind
    }

    public var artworkPath: [String] { ["api", "v1", "playlists", id, "artwork"] }
}

public struct PlaylistDetail: Equatable, Sendable {
    public let playlist: Playlist
    public let origin: String
    public let tracks: [Track]

    public var ready: Int { tracks.filter(\.isPlayable).count }
}

public struct Playlists: Sendable {
    struct Attribution: Decodable { let label: String }
    struct Origin: Decodable { let attribution: Attribution }

    struct Entry: Decodable {
        let entryId: String
        let recordingId: String?
        let title: String
        let artist: String
        let album: String?
        let releaseId: String?
        let durationMs: Int
        let available: Bool
        let playsFromServices: Bool
        let liked: Bool

        enum CodingKeys: String, CodingKey {
            case title, artist, album, available, liked
            case entryId = "entry_id"
            case recordingId = "recording_id"
            case releaseId = "release_id"
            case durationMs = "duration_ms"
            case playsFromServices = "plays_from_services"
        }

        var track: Track {
            Track(
                id: recordingId ?? "entry:\(entryId)",
                title: title,
                artist: artist,
                album: album,
                releaseId: releaseId,
                durationMs: durationMs,
                available: recordingId != nil && available,
                playsFromServices: recordingId != nil && playsFromServices,
                liked: liked
            )
        }
    }

    struct Detail: Decodable {
        let playlist: Playlist
        let entries: [Entry]
        let origin: Origin?
    }

    let api: HitoAPI

    public init(api: HitoAPI) {
        self.api = api
    }

    public func list() async throws -> [Playlist] {
        try await api.get(["api", "v1", "playlists"], as: [Playlist].self)
    }

    public func detail(_ id: String) async throws -> PlaylistDetail {
        let detail = try await api.get(["api", "v1", "playlists", id], as: Detail.self)
        let source = detail.origin?.attribution.label
        let origin = switch (detail.playlist.kind, source) {
        case (.followed, let source): "Followed from \(source ?? "a provider")"
        case (.local, .some(let source)): "A copy of \(source), yours to edit"
        case (.local, .none): "Your playlist"
        }
        return PlaylistDetail(playlist: detail.playlist, origin: origin, tracks: detail.entries.map(\.track))
    }
}
