import Foundation

public struct PlaylistDraft: Equatable, Sendable {
    public struct Slot: Equatable, Sendable {
        public let id: String
        public let position: Int

        public init(id: String, position: Int) {
            self.id = id
            self.position = position
        }
    }

    public enum Change: Equatable, Sendable {
        case rename(String)
        case remove(String)
        case move(String, to: Int)
    }

    struct Step: Equatable, Sendable {
        let id: String
        let delta: Int
    }

    static let largestReorder = 200

    public var name: String
    let savedName: String
    let saved: [Slot]
    var steps: [Step] = []
    public private(set) var removed: [String] = []

    public init(name: String, entries: [Slot]) {
        self.name = name
        savedName = name
        saved = entries
    }

    var kept: [Slot] { saved.filter { !removed.contains($0.id) } }

    var ordered: (slots: [Slot], moves: [Slot]) {
        Self.apply(kept, steps.filter { !removed.contains($0.id) }) ?? (kept, [])
    }

    public var entries: [String] { ordered.slots.map(\.id) }

    public mutating func remove(_ entry: String) {
        guard saved.contains(where: { $0.id == entry }), !removed.contains(entry) else { return }
        removed.append(entry)
    }

    @discardableResult public mutating func move(_ entry: String, by delta: Int) -> Bool {
        let next = steps + [Step(id: entry, delta: delta)]
        guard Self.apply(kept, next.filter { !removed.contains($0.id) }) != nil else { return false }
        steps = next
        return true
    }

    public var changes: [Change] {
        var result: [Change] = []
        let trimmed = name.trimmingCharacters(in: .whitespaces)
        if trimmed != savedName, !trimmed.isEmpty { result.append(.rename(trimmed)) }
        result += removed.map(Change.remove)
        result += ordered.moves.map { .move($0.id, to: $0.position) }
        return result
    }

    static func reorder(_ slots: [Slot], _ id: String, _ delta: Int) -> [Slot]? {
        guard let from = slots.firstIndex(where: { $0.id == id }) else { return [] }
        let to = from + delta
        guard slots.indices.contains(to) else { return [] }
        var order = slots.map(\.id)
        order.swapAt(from, to)
        var low = min(from, to)
        var high = max(from, to)
        while low > 0, slots[low - 1].position == slots[low].position { low -= 1 }
        while high < slots.count - 1, slots[high + 1].position == slots[high].position { high += 1 }
        var next: [(String, Int)] = []
        if Set(slots[low...high].map(\.position)).count == high - low + 1 {
            for index in low...high { next.append((order[index], slots[index].position)) }
        } else {
            var position = slots[low].position
            for index in low...high {
                next.append((order[index], position))
                position += 1
            }
            for index in (high + 1)..<max(high + 1, slots.count) {
                if slots[index].position >= position { break }
                next.append((order[index], position))
                position += 1
                if next.count > largestReorder { return nil }
            }
        }
        let current = Dictionary(uniqueKeysWithValues: slots.map { ($0.id, $0.position) })
        let moves = next.filter { current[$0.0] != $0.1 }.map { Slot(id: $0.0, position: $0.1) }
        return moves.count > largestReorder ? nil : moves
    }

    static func apply(_ slots: [Slot], _ steps: [Step]) -> (slots: [Slot], moves: [Slot])? {
        var working = slots
        for step in steps {
            guard let moves = reorder(working, step.id, step.delta) else { return nil }
            guard !moves.isEmpty, let from = working.firstIndex(where: { $0.id == step.id }) else { continue }
            working.swapAt(from, from + step.delta)
            let positions = Dictionary(uniqueKeysWithValues: moves.map { ($0.id, $0.position) })
            working = working.map { slot in positions[slot.id].map { Slot(id: slot.id, position: $0) } ?? slot }
        }
        let original = Dictionary(uniqueKeysWithValues: slots.map { ($0.id, $0.position) })
        return (working, working.filter { original[$0.id] != $0.position })
    }
}

extension Playlists {
    struct Edit: Encodable {
        let kind: String
        let playlistId: String
        let name: String?
        let entryId: String?
        let position: Int?

        enum CodingKeys: String, CodingKey {
            case kind, name, position
            case playlistId = "playlist_id"
            case entryId = "entry_id"
        }
    }

    struct EditChange: Encodable {
        let id: String
        let action: Edit
    }

    struct EditBatch: Encodable {
        let deviceId: String
        let operations: [EditChange]

        enum CodingKeys: String, CodingKey {
            case operations
            case deviceId = "device_id"
        }
    }

    func send(_ edits: [Edit], device: String) async throws {
        let changes = edits.map { EditChange(id: UUID().uuidString.lowercased(), action: $0) }
        let receipt = try await api.post(["api", "v1", "sync"], body: EditBatch(deviceId: device, operations: changes), as: Likes.Receipt.self)
        let acknowledged = Set(receipt.acknowledged.map { $0.lowercased() })
        guard changes.allSatisfy({ acknowledged.contains($0.id) }) else { throw Likes.Problem.notAcknowledged }
    }

    public func apply(_ changes: [PlaylistDraft.Change], to playlist: String, device: String) async throws {
        guard !changes.isEmpty else { return }
        try await send(changes.map { change in
            switch change {
            case let .rename(name): Edit(kind: "playlist_rename", playlistId: playlist, name: name, entryId: nil, position: nil)
            case let .remove(entry): Edit(kind: "entry_remove", playlistId: playlist, name: nil, entryId: entry, position: nil)
            case let .move(entry, position): Edit(kind: "entry_move", playlistId: playlist, name: nil, entryId: entry, position: position)
            }
        }, device: device)
    }

    public func create(named name: String, device: String) async throws -> String {
        let id = UUID().uuidString.lowercased()
        try await send([Edit(kind: "playlist_create", playlistId: id, name: name.trimmingCharacters(in: .whitespaces), entryId: nil, position: nil)], device: device)
        return id
    }

    public func delete(_ playlist: String, device: String) async throws {
        try await send([Edit(kind: "playlist_delete", playlistId: playlist, name: nil, entryId: nil, position: nil)], device: device)
    }
}
