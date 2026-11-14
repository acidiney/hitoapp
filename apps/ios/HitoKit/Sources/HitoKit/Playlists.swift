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

public enum FollowInterval: String, CaseIterable, Sendable {
    case hourly, daily

    public var label: String {
        switch self {
        case .hourly: "Every hour"
        case .daily: "Every day"
        }
    }
}

public enum PlaylistMode: Equatable, Sendable {
    case copy
    case follow(FollowInterval)
}

public enum PlaylistLink {
    public struct Problem: Error {}

    static let hosts = [
        "open.spotify.com": "spotify", "play.spotify.com": "spotify",
        "music.youtube.com": "youtube", "www.youtube.com": "youtube", "youtube.com": "youtube", "m.youtube.com": "youtube", "youtu.be": "youtube",
        "jamendo.com": "jamendo", "www.jamendo.com": "jamendo",
    ]

    static func keyword(_ reference: String) -> String? {
        let text = reference.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        if text.hasPrefix("spotify:") { return "spotify" }
        let url = URL(string: text.contains("://") ? text : "https://\(text)")
        return url?.host.flatMap { hosts[$0] }
    }

    public static func service(of reference: String) -> String? {
        keyword(reference).map { $0.prefix(1).uppercased() + $0.dropFirst() }
    }

    public static func connection(for reference: String, among connections: [PluginConnection]) -> PluginConnection? {
        let able = connections.filter { $0.capabilities.contains("playlists") }
        let matches = able.filter { connection in keyword(reference).map { connection.name.lowercased().contains($0) } ?? false }
        if matches.count == 1 { return matches[0] }
        return able.count == 1 && matches.isEmpty ? able[0] : nil
    }

    public static func problem(_ error: Error, service: String, following: Bool) -> String {
        let status: Int? = switch error as? HitoAPI.Problem {
        case let .refused(code), let .unexpected(code): code
        default: nil
        }
        switch status {
        case nil: return "Hito didn't get an answer. Trying again won't import it twice."
        case 401, 403: return "\(service) refused. Its connection may need your review. Nothing was saved."
        case 404: return "\(service) didn't return this playlist. It may be private, a personal mix, or deleted. Nothing was saved."
        case 429: return following ? "Hito can't follow more playlists right now. You can still copy this one." : "Hito can't import more playlists right now. Try again later."
        case 502, 503, 504: return "The \(service) plugin is busy. Nothing was saved. Try again in a moment."
        default: return "Hito couldn't import this playlist. Check the link and try again."
        }
    }
}

extension Playlists {
    struct Add: Encodable {
        let requestId: String
        let installationRevision: Int
        let playlistReference: String
        let interval: String?

        enum CodingKeys: String, CodingKey {
            case interval
            case requestId = "request_id"
            case installationRevision = "installation_revision"
            case playlistReference = "playlist_reference"
        }
    }

    struct Receipt: Decodable {
        let requestId: String
        let playlistId: String

        enum CodingKeys: String, CodingKey {
            case requestId = "request_id"
            case playlistId = "playlist_id"
        }
    }

    public func add(_ reference: String, from connection: PluginConnection, as mode: PlaylistMode, request: UUID = UUID()) async throws -> String {
        let id = request.uuidString.lowercased()
        let (path, interval): (String, String?) = switch mode {
        case .copy: ("import", nil)
        case let .follow(every): ("follow", every.rawValue)
        }
        let body = Add(requestId: id, installationRevision: connection.revision, playlistReference: reference.trimmingCharacters(in: .whitespacesAndNewlines), interval: interval)
        let receipt = try await api.post(["api", "v1", "plugins", connection.id, "playlists", path], body: body, as: Receipt.self)
        guard receipt.requestId.lowercased() == id else { throw PlaylistLink.Problem() }
        return receipt.playlistId
    }
}

extension Playlists {
    struct Insert: Encodable {
        let kind = "entry_insert"
        let playlistId: String
        let entryId: String
        let recordingId: String
        let position: Int

        enum CodingKeys: String, CodingKey {
            case kind, position
            case playlistId = "playlist_id"
            case entryId = "entry_id"
            case recordingId = "recording_id"
        }
    }

    struct Change: Encodable {
        let id: String
        let action: Insert
    }

    struct Changes: Encodable {
        let deviceId: String
        let operations: [Change]

        enum CodingKeys: String, CodingKey {
            case operations
            case deviceId = "device_id"
        }
    }

    public func insert(_ recordings: [String], into playlist: String, device: String, from start: Int) async throws {
        let changes = recordings.enumerated().map { index, recording in
            Change(id: UUID().uuidString.lowercased(), action: Insert(playlistId: playlist, entryId: UUID().uuidString.lowercased(), recordingId: recording, position: start + index))
        }
        let receipt = try await api.post(["api", "v1", "sync"], body: Changes(deviceId: device, operations: changes), as: Likes.Receipt.self)
        let acknowledged = Set(receipt.acknowledged.map { $0.lowercased() })
        guard changes.allSatisfy({ acknowledged.contains($0.id) }) else { throw Likes.Problem.notAcknowledged }
    }
}
