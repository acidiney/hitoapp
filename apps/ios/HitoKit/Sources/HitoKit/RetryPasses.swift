public struct RetryPasses: Sendable {
    public struct Pass: Equatable, Sendable {
        public let attempt: Int
        public let of: Int
        public let wait: Double
    }

    public static let count = 3
    static let waits: [Double] = [0.5, 1]

    public private(set) var attempt = 1

    public init() {}

    public mutating func next() -> Pass? {
        guard attempt < Self.count else { return nil }
        attempt += 1
        return Pass(attempt: attempt, of: Self.count, wait: Self.waits[attempt - 2])
    }
}
