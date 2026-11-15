public enum SourceFallback {
    public static let attempts = 6

    public static func allows(tried: Int, enabled: Bool) -> Bool {
        tried == 0 || (enabled && tried < attempts)
    }
}
