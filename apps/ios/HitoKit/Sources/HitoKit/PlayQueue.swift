public struct PlayQueue: Equatable, Sendable {
    public enum RepeatMode: Equatable, Sendable {
        case off, list, track
    }

    public struct Removed: Equatable, Sendable {
        let track: Track
        let queued: Bool
        let index: Int
    }

    public private(set) var list: [Track] = []
    public private(set) var position = 0
    public private(set) var queued: [Track] = []
    public private(set) var current: Track?
    public private(set) var context: String?
    public private(set) var shuffled = false
    public var repeatMode = RepeatMode.off
    private var original: [Track] = []

    public init() {}

    public var upNext: [Track] {
        guard position + 1 < list.count else { return [] }
        return Array(list[(position + 1)...])
    }

    public var hasNext: Bool { !queued.isEmpty || !upNext.isEmpty || (repeatMode == .list && !list.isEmpty) }

    public mutating func start(_ tracks: [Track], at index: Int, from context: String) {
        let chosen = tracks.indices.contains(index) ? tracks[index] : nil
        list = tracks.filter { $0.isPlayable || $0.id == chosen?.id }
        original = list
        position = chosen.flatMap { track in list.firstIndex { $0.id == track.id } } ?? 0
        queued = []
        shuffled = false
        current = list.indices.contains(position) ? list[position] : nil
        self.context = context
    }

    public mutating func setShuffle<Generator: RandomNumberGenerator>(_ on: Bool, using generator: inout Generator) {
        guard on != shuffled, let current, list.indices.contains(position) else { return }
        shuffled = on
        if on {
            original = list
            var others = list
            others.remove(at: position)
            others.shuffle(using: &generator)
            list = [current] + others
            position = 0
        } else {
            let playing = list[position]
            list = original
            position = list.firstIndex { $0.id == playing.id } ?? 0
        }
    }

    public mutating func playNext(_ track: Track) {
        queued.append(track)
    }

    @discardableResult
    public mutating func advance() -> Track? {
        if repeatMode == .track, let current { return current }
        if !queued.isEmpty {
            current = queued.removeFirst()
            return current
        }
        if position + 1 < list.count {
            position += 1
        } else if repeatMode == .list, !list.isEmpty {
            position = 0
        } else {
            return nil
        }
        current = list[position]
        return current
    }

    @discardableResult
    public mutating func back() -> Track? {
        guard position > 0 else { return nil }
        position -= 1
        current = list[position]
        return current
    }

    public mutating func clearUpcoming() {
        queued = []
        list = Array(list.prefix(position + 1))
        original = list
    }

    public mutating func move(upcoming id: String, by offset: Int) {
        if let index = queued.firstIndex(where: { $0.id == id }) {
            let target = min(max(index + offset, 0), queued.count - 1)
            queued.swapAt(index, target)
            return
        }
        guard let index = list.indices.dropFirst(position + 1).first(where: { list[$0].id == id }) else { return }
        let target = min(max(index + offset, position + 1), list.count - 1)
        list.swapAt(index, target)
    }

    @discardableResult
    public mutating func remove(upcoming id: String) -> Removed? {
        if let index = queued.firstIndex(where: { $0.id == id }) {
            return Removed(track: queued.remove(at: index), queued: true, index: index)
        }
        guard let index = list.indices.dropFirst(position + 1).first(where: { list[$0].id == id }) else { return nil }
        original.removeAll { $0.id == id }
        return Removed(track: list.remove(at: index), queued: false, index: index)
    }

    public mutating func restore(_ removed: Removed) {
        if removed.queued {
            queued.insert(removed.track, at: min(removed.index, queued.count))
        } else {
            list.insert(removed.track, at: min(max(removed.index, position + 1), list.count))
            if !shuffled { original = list } else { original.append(removed.track) }
        }
    }
}
