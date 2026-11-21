public struct StallWatch: Sendable {
    public typealias Instant = ContinuousClock.Instant

    public enum Engine: Equatable, Sendable {
        case playing
        case waiting
        case stopped
    }

    public enum Step: Equatable, Sendable {
        case rebuild
        case nextSource
        case giveUp
    }

    public enum Reach: Equatable, Sendable {
        case answered
        case unreachable
    }

    public static let deadline: Duration = .seconds(15)
    public static let serverWait: Duration = .seconds(120)

    public private(set) var engine = Engine.stopped
    public private(set) var due: Instant?
    private var rebuilt = false
    private var playingSince: Instant?
    private var unreachableSince: Instant?

    public init() {}

    public var buffering: Bool { engine == .waiting }

    public func rate(at speed: Double) -> Double {
        engine == .playing ? speed : 0
    }

    public mutating func observe(_ next: Engine, at now: Instant) {
        switch next {
        case .playing:
            if engine != .playing { playingSince = now }
            due = nil
            unreachableSince = nil
        case .waiting:
            guard engine != .waiting else { return }
            if let playingSince, now - playingSince >= Self.deadline { rebuilt = false }
            playingSince = nil
            due = now + Self.deadline
        case .stopped:
            playingSince = nil
            due = nil
            unreachableSince = nil
        }
        engine = next
    }

    public mutating func step(at now: Instant) -> Step? {
        guard let due, now >= due else { return nil }
        return escalate(at: now)
    }

    public mutating func failed(at now: Instant) -> Step {
        engine = .waiting
        playingSince = nil
        return escalate(at: now)
    }

    public mutating func rebuilt(_ reach: Reach, at now: Instant) {
        switch reach {
        case .answered:
            unreachableSince = nil
        case .unreachable:
            rebuilt = false
            if unreachableSince == nil { unreachableSince = now }
        }
        due = now + Self.deadline
    }

    private mutating func escalate(at now: Instant) -> Step {
        due = now + Self.deadline
        if let unreachableSince, now - unreachableSince >= Self.serverWait { return .giveUp }
        if rebuilt { return .nextSource }
        rebuilt = true
        return .rebuild
    }
}
