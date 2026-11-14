public struct PlayQueue: Equatable, Sendable {
    public private(set) var list: [Track] = []
    public private(set) var position = 0
    public private(set) var queued: [Track] = []
    public private(set) var current: Track?
    public private(set) var context: String?

    public init() {}

    public var upNext: [Track] {
        guard position + 1 < list.count else { return [] }
        return Array(list[(position + 1)...])
    }

    public var hasNext: Bool { !queued.isEmpty || !upNext.isEmpty }

    public mutating func start(_ tracks: [Track], at index: Int, from context: String) {
        let chosen = tracks.indices.contains(index) ? tracks[index] : nil
        list = tracks.filter { $0.isPlayable || $0.id == chosen?.id }
        position = chosen.flatMap { track in list.firstIndex { $0.id == track.id } } ?? 0
        queued = []
        current = list.indices.contains(position) ? list[position] : nil
        self.context = context
    }

    public mutating func playNext(_ track: Track) {
        queued.append(track)
    }

    @discardableResult
    public mutating func advance() -> Track? {
        if !queued.isEmpty {
            current = queued.removeFirst()
            return current
        }
        guard position + 1 < list.count else { return nil }
        position += 1
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
    }

    public mutating func remove(upcoming id: String) {
        if let index = queued.firstIndex(where: { $0.id == id }) {
            queued.remove(at: index)
            return
        }
        guard let index = list.indices.dropFirst(position + 1).first(where: { list[$0].id == id }) else { return }
        list.remove(at: index)
    }
}
