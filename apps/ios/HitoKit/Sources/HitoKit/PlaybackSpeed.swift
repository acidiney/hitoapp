public enum PlaybackSpeed {
    public static let choices: [Double] = [0.5, 0.75, 1, 1.25, 1.5, 2]

    public static func label(_ speed: Double) -> String {
        let whole = speed.rounded() == speed
        return "\(whole ? String(Int(speed)) : String(speed))×"
    }
}
