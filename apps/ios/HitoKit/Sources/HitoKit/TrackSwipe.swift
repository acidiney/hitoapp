public struct TrackSwipe: Equatable, Sendable {
    public enum Turn: Equatable, Sendable {
        case next
        case previous
    }

    public static let turningShare = 0.3
    public static let flickSpeed = 600.0
    public static let flickDistance = 40.0
    public static let pushBackSpeed = 300.0
    public static let edgeResistance = 0.2

    public let hasNext: Bool
    public let hasPrevious: Bool
    public private(set) var translation = 0.0

    public init(hasNext: Bool, hasPrevious: Bool) {
        self.hasNext = hasNext
        self.hasPrevious = hasPrevious
    }

    public mutating func drag(x: Double) {
        translation = x
    }

    public var slides: Bool {
        translation < 0 ? hasNext : hasPrevious
    }

    public var offset: Double {
        slides ? translation : translation * Self.edgeResistance
    }

    public func turn(velocity: Double, width: Double) -> Turn? {
        guard translation != 0 else { return nil }
        let forward = translation < 0
        let distance = abs(translation)
        let speed = forward ? -velocity : velocity
        guard speed > -Self.pushBackSpeed else { return nil }
        let far = distance >= width * Self.turningShare || (speed >= Self.flickSpeed && distance >= Self.flickDistance)
        guard far else { return nil }
        if forward { return hasNext ? .next : nil }
        return .previous
    }
}
