import Foundation

public struct Announcement: Equatable, Sendable {
    public static let limit = 2
    public static let stopped = Announcement(current: nil, upcoming: [])

    public let current: String?
    public let upcoming: [String]

    public init(current: String?, upcoming: [String]) {
        self.current = current
        self.upcoming = Array(upcoming.prefix(Self.limit))
    }

    public init(queue: PlayQueue, playing: Bool) {
        guard playing, let current = queue.current else {
            self = .stopped
            return
        }
        self.init(current: current.id, upcoming: queue.upcoming.map(\.id))
    }
}

public struct AnnouncementPace: Sendable {
    public typealias Instant = ContinuousClock.Instant

    public enum Outcome: Equatable, Sendable {
        case delivered
        case limited(retryAfter: Duration?)
        case rejected
        case unsupported
        case failed
    }

    public static let settle: Duration = .seconds(1)
    public static let spacing: Duration = .seconds(2)
    public static let limitedWait: Duration = .seconds(60)
    public static let failureWait: Duration = .seconds(30)

    private var wanted = Announcement.stopped
    private var changedAt: Instant?
    private var announced = Announcement.stopped
    private var sending: Announcement?
    private var earliest: Instant?
    private var blocked: Instant?
    private var unsupported = false

    public init() {}

    public var due: Instant? {
        guard !unsupported, sending == nil, wanted != announced, let changedAt else { return nil }
        return [earliest, blocked].compactMap { $0 }.reduce(changedAt + Self.settle, max)
    }

    public mutating func want(_ next: Announcement, at now: Instant) {
        guard next != wanted else { return }
        wanted = next
        changedAt = now
    }

    public mutating func take(at now: Instant) -> Announcement? {
        guard let due, now >= due else { return nil }
        sending = wanted
        earliest = now + Self.spacing
        return wanted
    }

    public mutating func flush(_ last: Announcement, at now: Instant) -> Announcement? {
        want(last, at: now)
        guard !unsupported, sending == nil, wanted != announced else { return nil }
        if let blocked, now < blocked { return nil }
        sending = wanted
        earliest = now + Self.spacing
        return wanted
    }

    public mutating func finished(_ outcome: Outcome, at now: Instant) {
        guard let sent = sending else { return }
        sending = nil
        switch outcome {
        case .delivered, .rejected:
            announced = sent
        case let .limited(retryAfter):
            blocked = now + (retryAfter ?? Self.limitedWait)
        case .unsupported:
            unsupported = true
        case .failed:
            blocked = now + Self.failureWait
        }
    }
}

public struct UpcomingTracks: Sendable {
    struct Body: Encodable {
        let client: String
        let current: String?
        let recordingIds: [String]

        enum CodingKeys: String, CodingKey {
            case client, current
            case recordingIds = "recording_ids"
        }

        func encode(to encoder: Encoder) throws {
            var values = encoder.container(keyedBy: CodingKeys.self)
            try values.encode(client, forKey: .client)
            try values.encode(current, forKey: .current)
            try values.encode(recordingIds, forKey: .recordingIds)
        }
    }

    let api: HitoAPI
    let client: String

    public init(api: HitoAPI, client: String) {
        self.api = api
        self.client = client
    }

    static func tagKey(_ account: String) -> String { "app.hito.upcoming-client.\(account)" }

    public static func clientTag(defaults: UserDefaults = .standard, account: String) -> String {
        if let saved = defaults.string(forKey: tagKey(account)) { return saved }
        let made = "ios-\(UUID().uuidString.lowercased())"
        defaults.set(made, forKey: tagKey(account))
        return made
    }

    public func announce(_ announcement: Announcement) async -> AnnouncementPace.Outcome {
        let body = Body(client: client, current: announcement.current, recordingIds: announcement.upcoming)
        let response: HTTPURLResponse
        do {
            response = try await api.answer("POST", ["api", "v1", "playback", "upcoming"], body: body)
        } catch {
            return .failed
        }
        switch response.statusCode {
        case 200..<300:
            return .delivered
        case 429:
            let wait = response.value(forHTTPHeaderField: "Retry-After").flatMap(Int.init).map { Duration.seconds($0) }
            return .limited(retryAfter: wait)
        case 404, 405:
            return .unsupported
        case 400, 413, 415, 422:
            return .rejected
        default:
            return .failed
        }
    }
}

public struct KeepPlayedTracks: Equatable, Sendable {
    public let on: Bool
    public let usedBytes: Int
    public let budgetBytes: Int

    public init(on: Bool, usedBytes: Int, budgetBytes: Int) {
        self.on = on
        self.usedBytes = usedBytes
        self.budgetBytes = budgetBytes
    }

    public var space: String {
        guard budgetBytes > 0 else { return "Your administrator has set no space for played tracks." }
        return "\(Self.size(usedBytes)) of \(Self.size(budgetBytes))"
    }

    static func size(_ bytes: Int) -> String {
        let gibibyte = 1024.0 * 1024 * 1024
        let value = Double(bytes)
        guard value >= gibibyte else { return "\(Int((value / (1024 * 1024)).rounded())) MiB" }
        let tenths = (value / gibibyte * 10).rounded() / 10
        return tenths == tenths.rounded() ? "\(Int(tenths)) GiB" : String(format: "%.1f GiB", tenths)
    }
}

public struct AutomaticCopies: Sendable {
    struct Wire: Decodable {
        let keepPlayedTracks: Bool
        let usedBytes: Int
        let budgetBytes: Int

        enum CodingKeys: String, CodingKey {
            case keepPlayedTracks = "keep_played_tracks"
            case usedBytes = "used_bytes"
            case budgetBytes = "budget_bytes"
        }

        var setting: KeepPlayedTracks { KeepPlayedTracks(on: keepPlayedTracks, usedBytes: usedBytes, budgetBytes: budgetBytes) }
    }

    struct Preference: Encodable {
        let keepPlayedTracks: Bool

        enum CodingKeys: String, CodingKey { case keepPlayedTracks = "keep_played_tracks" }
    }

    static let path = ["api", "v1", "account", "automatic-copies"]

    let api: HitoAPI

    public init(api: HitoAPI) {
        self.api = api
    }

    public func load() async throws -> KeepPlayedTracks? {
        do {
            return try await api.get(Self.path, as: Wire.self).setting
        } catch HitoAPI.Problem.refused(404) {
            return nil
        }
    }

    public func set(_ on: Bool) async throws -> KeepPlayedTracks {
        try await api.put(Self.path, body: Preference(keepPlayedTracks: on), as: Wire.self).setting
    }
}
