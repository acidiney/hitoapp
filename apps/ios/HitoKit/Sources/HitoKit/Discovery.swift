import Foundation

public struct RadioArtist: Equatable, Hashable, Identifiable, Sendable {
    public let id: String
    public let name: String
    public let tracks: Int
}

public struct ArtistRadio: Equatable, Sendable {
    public let artist: String
    public let similar: Int
    public let tracks: [Track]

    public var reach: String {
        let own = tracks.filter { $0.artist == artist }.count
        let count = own > 0 ? own : tracks.count
        let ready = "\(count) ready \(count == 1 ? "track" : "tracks")"
        if similar == 0 { return "Starts with \(ready) of \(artist)." }
        return "Starts with \(ready), then \(similar) nearby \(similar == 1 ? "artist" : "artists") in your library."
    }
}

public struct FeaturedItem: Equatable, Identifiable, Sendable {
    public let id: String
    public let title: String
    public let detail: String
    public let installation: String
    public let revision: Int
    public let reference: String
    public let playlist: String?
}

public enum FeaturedShelf: Equatable, Sendable {
    case available([FeaturedItem])
    case off
}

public struct Discovery: Sendable {
    struct Name: Decodable { let value: String }

    struct Group: Decodable {
        let id: String
        let name: Name
        let recordingCount: Int

        enum CodingKeys: String, CodingKey {
            case id, name
            case recordingCount = "recording_count"
        }
    }

    struct Groups: Decodable {
        let items: [Group]
        let total: Int
    }

    struct Radio: Decodable {
        let artist: String
        let similarArtists: Int
        let tracks: [Track]

        enum CodingKeys: String, CodingKey {
            case artist, tracks
            case similarArtists = "similar_artists"
        }
    }

    struct Entry: Decodable {
        let installationId: String
        let installationRevision: Int
        let provider: String
        let id: String
        let title: String
        let subtitle: String?
        let reference: String
        let existingPlaylistId: String?

        enum CodingKeys: String, CodingKey {
            case provider, id, title, subtitle, reference
            case installationId = "installation_id"
            case installationRevision = "installation_revision"
            case existingPlaylistId = "existing_playlist_id"
        }
    }

    struct Shelf: Decodable {
        let status: String
        let items: [Entry]?
    }

    struct Open: Encodable {
        let requestId: String
        let installationRevision: Int
        let playlistReference: String

        enum CodingKeys: String, CodingKey {
            case requestId = "request_id"
            case installationRevision = "installation_revision"
            case playlistReference = "playlist_reference"
        }
    }

    struct Opened: Decodable {
        let requestId: String
        let playlistId: String

        enum CodingKeys: String, CodingKey {
            case requestId = "request_id"
            case playlistId = "playlist_id"
        }
    }

    let api: HitoAPI

    public init(api: HitoAPI) {
        self.api = api
    }

    public func artists() async throws -> RadioArtists {
        let page = try await api.get(["api", "v1", "artist-identity-groups"], query: [URLQueryItem(name: "limit", value: "24")], as: Groups.self)
        let items = page.items
            .map { RadioArtist(id: $0.id, name: $0.name.value, tracks: $0.recordingCount) }
            .sorted { $0.tracks > $1.tracks }
        return RadioArtists(items: items, complete: page.total <= page.items.count)
    }

    public func radio(_ id: String) async throws -> ArtistRadio {
        let radio = try await api.post(["api", "v1", "artist-identity-groups", id, "radio"], body: Optional<String>.none, as: Radio.self)
        return ArtistRadio(artist: radio.artist, similar: radio.similarArtists, tracks: radio.tracks)
    }

    public func featured() async throws -> FeaturedShelf {
        let shelf = try await api.get(["api", "v1", "discover", "featured"], as: Shelf.self)
        guard shelf.status == "available", let items = shelf.items else { return .off }
        return .available(items.map { entry in
            FeaturedItem(
                id: entry.id,
                title: entry.title,
                detail: [entry.subtitle, entry.provider].compactMap { $0 }.filter { !$0.isEmpty }.joined(separator: " · "),
                installation: entry.installationId,
                revision: entry.installationRevision,
                reference: entry.reference,
                playlist: entry.existingPlaylistId
            )
        })
    }

    public func open(_ item: FeaturedItem, request: UUID = UUID()) async throws -> String {
        if let playlist = item.playlist { return playlist }
        let id = request.uuidString.lowercased()
        let opened = try await api.post(["api", "v1", "plugins", item.installation, "playlists", "open"], body: Open(requestId: id, installationRevision: item.revision, playlistReference: item.reference), as: Opened.self)
        guard opened.requestId.lowercased() == id else { throw PlaylistLink.Problem() }
        return opened.playlistId
    }
}

public struct RadioArtists: Equatable, Sendable {
    public let items: [RadioArtist]
    public let complete: Bool

    public var heroLabel: String {
        complete ? "Radio · because you own the most of this artist" : "Radio · from an artist in your library"
    }
}
