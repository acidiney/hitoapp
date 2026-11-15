import Foundation

public struct AudiobookAnchor: Codable, Equatable, Hashable, Sendable {
    public let audiobookId: String
    public let entryId: String
    public let recordingId: String

    public init(audiobookId: String, entryId: String, recordingId: String) {
        self.audiobookId = audiobookId
        self.entryId = entryId
        self.recordingId = recordingId
    }

    enum CodingKeys: String, CodingKey {
        case audiobookId = "audiobook_id"
        case entryId = "entry_id"
        case recordingId = "recording_id"
    }
}

public struct Audiobooks: Sendable {
    public struct Shelf: Equatable, Sendable {
        public let books: [Playlist]
        public let next: String?
    }

    public struct Chapter: Equatable, Identifiable, Sendable {
        public let entryId: String
        public let track: Track

        public var id: String { entryId }
    }

    public struct Bookmark: Equatable, Identifiable, Sendable {
        public let id: String
        public let label: String
        public let positionMs: Int
        public let chapterIndex: Int
    }

    public struct Continuation: Equatable, Sendable {
        public let index: Int
        public let positionMs: Int
    }

    public struct Overall: Equatable, Sendable {
        public let fraction: Double
        public let left: String
    }

    public struct Book: Equatable, Sendable {
        public let playlist: Playlist
        public let chapters: [Chapter]
        public let bookmarks: [Bookmark]
        public let continuation: Continuation?

        public var anchors: [AudiobookAnchor] {
            chapters.map { AudiobookAnchor(audiobookId: playlist.id, entryId: $0.entryId, recordingId: $0.track.id) }
        }

        public var continueLabel: String? {
            guard let continuation else { return nil }
            return "Continue ch. \(continuation.index + 1) · \(SourceAttachment.clock(continuation.positionMs))"
        }

        public var overall: Overall? {
            guard let continuation, chapters.allSatisfy({ $0.track.durationMs > 0 }) else { return nil }
            let total = chapters.reduce(0) { $0 + $1.track.durationMs }
            let before = chapters.prefix(continuation.index).reduce(0) { $0 + $1.track.durationMs }
            let done = before + min(continuation.positionMs, chapters[continuation.index].track.durationMs)
            return Overall(fraction: Double(done) / Double(total), left: "\(Podcasts.Episode.long(max(0, total - done))) left")
        }
    }

    struct Pagination: Decodable {
        let nextCursor: String?

        enum CodingKeys: String, CodingKey { case nextCursor = "next_cursor" }
    }

    struct ShelfWire: Decodable {
        let items: [Playlist]
        let pagination: Pagination
    }

    struct BookmarkWire: Decodable {
        let id: String
        let entryId: String
        let positionMs: Int
        let label: String

        enum CodingKeys: String, CodingKey {
            case id, label
            case entryId = "entry_id"
            case positionMs = "position_ms"
        }
    }

    struct Progress: Decodable {
        let positionMs: Int

        enum CodingKeys: String, CodingKey { case positionMs = "position_ms" }
    }

    struct ContinuationWire: Decodable {
        let chapter: AudiobookAnchor
        let progress: Progress?
    }

    struct Detail: Decodable {
        let playlist: Playlist
        let chapters: [Playlists.Entry]
        let bookmarks: [BookmarkWire]
        let continuation: ContinuationWire?
    }

    struct Create: Encodable {
        let kind = "audiobook_create"
        let playlistId: String

        enum CodingKeys: String, CodingKey {
            case kind
            case playlistId = "playlist_id"
        }
    }

    struct BookmarkCreate: Encodable {
        let kind = "audiobook_bookmark_create"
        let audiobookId: String
        let bookmarkId: String
        let entryId: String
        let recordingId: String
        let positionMs: Int
        let label: String

        enum CodingKeys: String, CodingKey {
            case kind, label
            case audiobookId = "audiobook_id"
            case bookmarkId = "bookmark_id"
            case entryId = "entry_id"
            case recordingId = "recording_id"
            case positionMs = "position_ms"
        }
    }

    struct BookmarkRemove: Encodable {
        let kind = "audiobook_bookmark_remove"
        let audiobookId: String
        let bookmarkId: String

        enum CodingKeys: String, CodingKey {
            case kind
            case audiobookId = "audiobook_id"
            case bookmarkId = "bookmark_id"
        }
    }

    struct Change<Action: Encodable>: Encodable {
        let id: String
        let action: Action
    }

    struct Batch<Action: Encodable>: Encodable {
        let deviceId: String
        let operations: [Change<Action>]

        enum CodingKeys: String, CodingKey {
            case operations
            case deviceId = "device_id"
        }
    }

    static let shelfSize = 24

    let api: HitoAPI

    public init(api: HitoAPI) {
        self.api = api
    }

    public func books(cursor: String?) async throws -> Shelf {
        var query = [URLQueryItem(name: "limit", value: String(Self.shelfSize))]
        if let cursor { query.append(URLQueryItem(name: "cursor", value: cursor)) }
        let wire = try await api.get(["api", "v1", "audiobooks", "page"], query: query, as: ShelfWire.self)
        return Shelf(books: wire.items, next: wire.pagination.nextCursor)
    }

    public func detail(_ id: String) async throws -> Book {
        let wire = try await api.get(["api", "v1", "audiobooks", id], as: Detail.self)
        let chapters = wire.chapters.map { Chapter(entryId: $0.entryId, track: $0.track) }
        let index = { (entry: String) in chapters.firstIndex { $0.entryId == entry } }
        let continuation = wire.continuation.flatMap { found in
            index(found.chapter.entryId).flatMap { position in found.progress.map { Continuation(index: position, positionMs: $0.positionMs) } }
        }
        let bookmarks = wire.bookmarks.compactMap { mark in
            index(mark.entryId).map { Bookmark(id: mark.id, label: mark.label, positionMs: mark.positionMs, chapterIndex: $0) }
        }
        return Book(playlist: wire.playlist, chapters: chapters, bookmarks: bookmarks, continuation: continuation)
    }

    public func create(from playlist: String, device: String) async throws {
        try await send(Create(playlistId: playlist), device: device)
    }

    public func bookmark(_ anchor: AudiobookAnchor, at positionMs: Int, label: String, device: String) async throws -> String {
        let id = UUID().uuidString.lowercased()
        try await send(BookmarkCreate(audiobookId: anchor.audiobookId, bookmarkId: id, entryId: anchor.entryId, recordingId: anchor.recordingId, positionMs: positionMs, label: label), device: device)
        return id
    }

    public func removeBookmark(_ bookmark: String, from audiobook: String, device: String) async throws {
        try await send(BookmarkRemove(audiobookId: audiobook, bookmarkId: bookmark), device: device)
    }

    func send<Action: Encodable>(_ action: Action, device: String) async throws {
        let change = Change(id: UUID().uuidString.lowercased(), action: action)
        let receipt = try await api.post(["api", "v1", "sync"], body: Batch(deviceId: device, operations: [change]), as: Likes.Receipt.self)
        guard receipt.acknowledged.map({ $0.lowercased() }).contains(change.id) else { throw Likes.Problem.notAcknowledged }
    }
}
