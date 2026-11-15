import Foundation

public struct StoredQueue: Codable, Equatable, Sendable {
    public let ids: [String]
    public let index: Int
    public let position: Double
    public let context: String

    public struct Restored: Equatable, Sendable {
        public let tracks: [Track]
        public let index: Int
        public let position: Double
        public let context: String
    }

    public func restore(_ found: [Track]) -> Restored? {
        let byId = Dictionary(found.map { ($0.id, $0) }, uniquingKeysWith: { first, _ in first })
        var tracks: [Track] = []
        var current: Int?
        var before = 0
        for (at, id) in ids.enumerated() {
            guard let track = byId[id] else {
                if at < index { before += 1 }
                continue
            }
            if at == index { current = tracks.count }
            tracks.append(track)
        }
        let place = current ?? min(index - before, tracks.count - 1)
        guard place >= 0, !tracks.isEmpty else { return nil }
        let length = Double(tracks[place].durationMs) / 1000
        let resumed = current == nil ? 0 : length > 0 ? min(position, length) : position
        return Restored(tracks: tracks, index: place, position: resumed, context: context)
    }
}

public struct QueueMemory: @unchecked Sendable {
    static let limit = 500
    static let lookbehind = 50
    private let defaults: UserDefaults
    private let key: String

    public init(defaults: UserDefaults = .standard, account: String) {
        self.defaults = defaults
        key = "app.hito.queue.v1.\(account)"
    }

    public func save(ids: [String], index: Int, position: Double, context: String) {
        guard ids.indices.contains(index) else { return forget() }
        let first = max(0, index - Self.lookbehind)
        let window = Array(ids[first..<min(ids.count, first + Self.limit)])
        let stored = StoredQueue(ids: window, index: index - first, position: position.isFinite ? position : 0, context: context)
        guard let data = try? JSONEncoder().encode(stored) else { return }
        defaults.set(data, forKey: key)
    }

    public func load() -> StoredQueue? {
        guard let data = defaults.data(forKey: key), let stored = try? JSONDecoder().decode(StoredQueue.self, from: data),
              stored.ids.indices.contains(stored.index) else { return nil }
        return stored
    }

    public func forget() {
        defaults.removeObject(forKey: key)
    }
}

extension Library {
    struct Lookup: Encodable {
        let recordingIds: [String]

        enum CodingKeys: String, CodingKey { case recordingIds = "recording_ids" }
    }

    public func lookup(_ ids: [String]) async throws -> [Track] {
        try await api.post(["api", "v1", "recordings", "lookup"], body: Lookup(recordingIds: ids), as: [Track].self)
    }
}
