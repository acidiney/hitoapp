import Foundation

public enum StreamingQuality: String, CaseIterable, Sendable {
    case original
    case high
    case saver

    public var title: String {
        switch self {
        case .original: "Original"
        case .high: "High"
        case .saver: "Data saver"
        }
    }

    static func key(_ account: String) -> String { "app.hito.streaming-quality.\(account)" }

    public static func preferred(defaults: UserDefaults = .standard, account: String) -> StreamingQuality {
        defaults.string(forKey: key(account)).flatMap(StreamingQuality.init(rawValue:)) ?? .original
    }

    public static func setPreferred(_ quality: StreamingQuality, defaults: UserDefaults = .standard, account: String) {
        defaults.set(quality.rawValue, forKey: key(account))
    }
}

public struct StreamChoice: Equatable, Sendable {
    public let url: URL
    public let quality: StreamingQuality
    public let startOffset: Double
    public let fallback: String?

    public static let originalOnly = "This track plays at original quality"

    public init(url: URL, quality: StreamingQuality, startOffset: Double, fallback: String?) {
        self.url = url
        self.quality = quality
        self.startOffset = startOffset
        self.fallback = fallback
    }
}

extension Playback {
    public func stream(_ request: AudioRequest, quality: StreamingQuality, from seconds: Double) async throws -> StreamChoice {
        let original = StreamChoice(url: request.url, quality: .original, startOffset: 0, fallback: nil)
        guard quality != .original else { return original }
        let startMs = max(0, Int((seconds * 1000).rounded(.down)))
        let query = [
            URLQueryItem(name: "quality", value: quality.rawValue),
            URLQueryItem(name: "start_ms", value: String(startMs)),
        ]
        let path = ["api", "v1", "playback", "leases", request.leaseId, "audio"]
        let response = try await api.head(path, query: query)
        let fallback = StreamChoice(url: request.url, quality: .original, startOffset: 0, fallback: StreamChoice.originalOnly)
        switch response.statusCode {
        case 200: break
        case 403, 404: throw HitoAPI.Problem.refused(response.statusCode)
        default: return fallback
        }
        guard response.value(forHTTPHeaderField: "X-Hito-Stream-Quality") == quality.rawValue,
              let confirmed = response.value(forHTTPHeaderField: "X-Hito-Stream-Start-Ms").flatMap(Int.init),
              confirmed == startMs,
              let url = api.request(path, query: query).url
        else { return fallback }
        return StreamChoice(url: url, quality: quality, startOffset: Double(confirmed) / 1000, fallback: nil)
    }
}
