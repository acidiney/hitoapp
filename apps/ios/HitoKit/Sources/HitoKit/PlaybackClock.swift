public enum PlaybackClock {
    static let tolerance = 0.15

    static func close(_ value: Double, to listed: Double) -> Bool {
        abs(value - listed) <= listed * tolerance
    }

    public static func length(stream: Double, listed: Double) -> Double {
        guard listed > 0 else { return stream }
        if close(stream, to: listed) { return stream }
        if close(stream / 2, to: listed) { return stream / 2 }
        return listed
    }

    public static func pastTheEnd(at position: Double, length: Double, stream: Double) -> Bool {
        stream > length + 1 && position >= length
    }
}
