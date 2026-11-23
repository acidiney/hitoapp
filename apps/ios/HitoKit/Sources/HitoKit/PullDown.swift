public struct PullDown: Equatable, Sendable {
    public enum Axis: Equatable, Sendable {
        case vertical
        case horizontal
    }

    public static let directionDistance = 10.0
    public static let closingShare = 0.25
    public static let flickSpeed = 800.0
    public static let flickDistance = 24.0
    public static let pushBackSpeed = -300.0

    public private(set) var axis: Axis?
    public private(set) var offset = 0.0

    public init() {}

    public mutating func drag(x: Double, y: Double) {
        if axis == nil, max(abs(x), abs(y)) >= Self.directionDistance {
            axis = abs(y) > abs(x) ? .vertical : .horizontal
        }
        offset = axis == .vertical ? max(y, 0) : 0
    }

    public func closes(velocity: Double, height: Double) -> Bool {
        guard axis == .vertical, offset > 0, velocity > Self.pushBackSpeed else { return false }
        if offset >= height * Self.closingShare { return true }
        return velocity >= Self.flickSpeed && offset >= Self.flickDistance
    }
}
