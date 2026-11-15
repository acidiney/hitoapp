import Foundation

public struct ArtistPage: Sendable {
    public struct Detail: Equatable, Sendable {
        public let id: String
        public let name: String
        public let stats: String
        public let tracks: [Track]
        public let tracksTotal: Int
        public let albums: [Album]
        public let appearances: [Track]
    }

    struct Wire: Decodable {
        struct Head: Decodable {
            let id: String
            let name: String
            let recordingCount: Int
            let releaseCount: Int
            let featuredRecordingCount: Int?

            enum CodingKeys: String, CodingKey {
                case id, name
                case recordingCount = "recording_count"
                case releaseCount = "release_count"
                case featuredRecordingCount = "featured_recording_count"
            }
        }

        struct Slice<Item: Decodable>: Decodable {
            let items: [Item]
            let total: Int
        }

        let artist: Head
        let recordings: Slice<Track>
        let releases: Slice<Album>
        let featuredRecordings: Slice<Track>?

        enum CodingKeys: String, CodingKey {
            case artist, recordings, releases
            case featuredRecordings = "featured_recordings"
        }
    }

    let api: HitoAPI

    public init(api: HitoAPI) {
        self.api = api
    }

    public func load(_ artist: String) async throws -> Detail {
        let wire = try await api.get(["api", "v1", "artists", artist], as: Wire.self)
        return Detail(
            id: wire.artist.id,
            name: wire.artist.name,
            stats: Self.counts(tracks: wire.artist.recordingCount, albums: wire.artist.releaseCount, featured: wire.artist.featuredRecordingCount ?? 0),
            tracks: wire.recordings.items,
            tracksTotal: wire.recordings.total,
            albums: wire.releases.items,
            appearances: wire.featuredRecordings?.items ?? []
        )
    }

    public static func counts(tracks: Int, albums: Int, featured: Int) -> String {
        func count(_ value: Int, _ one: String, _ many: String) -> String { "\(value) \(value == 1 ? one : many)" }
        let parts = [
            tracks > 0 ? count(tracks, "track", "tracks") : nil,
            albums > 0 ? count(albums, "album", "albums") + " in your library" : nil,
            featured > 0 ? count(featured, "appearance", "appearances") : nil,
        ].compactMap { $0 }
        return parts.isEmpty ? "0 tracks" : parts.joined(separator: " · ")
    }
}
