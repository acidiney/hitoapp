public struct Lookahead: Sendable {
    public enum Miss: Equatable, Sendable {
        case noSource(retryable: Bool)
        case inconclusive

        public init(_ error: any Error) {
            switch error {
            case let Playback.Problem.notFound(_, retryable):
                self = .noSource(retryable: retryable)
            case Playback.Problem.noReadySource:
                self = .noSource(retryable: false)
            default:
                self = .inconclusive
            }
        }
    }

    public enum Move: Equatable, Sendable {
        case ended
        case chosen
    }

    private var retried: Set<String> = []
    private var unplayable: Set<String> = []
    private var inconclusive: Set<String> = []

    public init() {}

    public func target(_ upcoming: [String]) -> String? {
        guard let first = upcoming.first(where: { !unplayable.contains($0) }), !inconclusive.contains(first) else { return nil }
        return first
    }

    public mutating func missed(_ id: String, _ miss: Miss) -> Double? {
        switch miss {
        case .noSource(retryable: true) where !retried.contains(id):
            retried.insert(id)
            var passes = RetryPasses()
            return passes.next()?.wait
        case .noSource:
            unplayable.insert(id)
        case .inconclusive:
            inconclusive.insert(id)
        }
        return nil
    }

    public mutating func found(_ id: String) {
        retried.remove(id)
        unplayable.remove(id)
        inconclusive.remove(id)
    }

    public func passing(_ upcoming: [String], on move: Move) -> Int {
        guard move == .ended else { return 0 }
        return upcoming.prefix { unplayable.contains($0) }.count
    }

    public static func notice(_ titles: [String]) -> String {
        switch titles.count {
        case 1: "\(titles[0]) has no source that plays right now."
        case 2: "\(titles[0]) and \(titles[1]) have no source that plays right now."
        default: "\(titles[0]) and \(titles.count - 1) more tracks have no source that plays right now."
        }
    }
}
