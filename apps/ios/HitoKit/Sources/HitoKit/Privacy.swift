import Foundation

public struct PrivacySettings: Equatable, Sendable {
    public var pinEnabled: Bool
    public var idleMinutes: Int
    public var history: Bool
    public var artistLookup: Bool
    public var artwork: Bool

    public init(pinEnabled: Bool, idleMinutes: Int, history: Bool, artistLookup: Bool, artwork: Bool) {
        self.pinEnabled = pinEnabled
        self.idleMinutes = idleMinutes
        self.history = history
        self.artistLookup = artistLookup
        self.artwork = artwork
    }
}

public struct Privacy: Sendable {
    public enum Lookup: String, Sendable {
        case artistLookup = "automatic_artist_lookup"
        case artwork = "automatic_artwork"
    }

    struct Lock: Decodable {
        let pinEnabled: Bool
        let idleMinutes: Int

        enum CodingKeys: String, CodingKey {
            case pinEnabled = "pin_enabled"
            case idleMinutes = "idle_minutes"
        }
    }

    struct SetLock: Encodable {
        let pin: String?
        let idleMinutes: Int
        let password: String

        enum CodingKeys: String, CodingKey {
            case pin, password
            case idleMinutes = "idle_minutes"
        }

        func encode(to encoder: Encoder) throws {
            var container = encoder.container(keyedBy: CodingKeys.self)
            try container.encode(pin, forKey: .pin)
            try container.encode(idleMinutes, forKey: .idleMinutes)
            try container.encode(password, forKey: .password)
        }
    }

    public struct LockState: Equatable, Sendable {
        public let pinEnabled: Bool
        public let idleMinutes: Int
    }

    public static let idleChoices = [0, 5, 15, 60]

    struct History: Codable { let enabled: Bool }

    public struct Lookups: Decodable, Sendable {
        public let artistLookup: Bool
        public let artwork: Bool

        enum CodingKeys: String, CodingKey {
            case artistLookup = "automatic_artist_lookup"
            case artwork = "automatic_artwork"
        }
    }

    struct Cleared: Decodable {}

    let api: HitoAPI

    public init(api: HitoAPI) {
        self.api = api
    }

    public func load() async throws -> PrivacySettings {
        async let lock = api.get(["api", "v1", "account", "lock"], as: Lock.self)
        async let history = api.get(["api", "v1", "history", "preferences"], as: History.self)
        async let lookups = api.get(["api", "v1", "metadata", "settings"], as: Lookups.self)
        let (pin, saved, automatic) = try await (lock, history, lookups)
        return PrivacySettings(pinEnabled: pin.pinEnabled, idleMinutes: pin.idleMinutes, history: saved.enabled, artistLookup: automatic.artistLookup, artwork: automatic.artwork)
    }

    public func saveLock(pin: String?, idleMinutes: Int, password: String) async throws -> LockState {
        let lock = try await api.put(["api", "v1", "account", "pin"], body: SetLock(pin: pin, idleMinutes: idleMinutes, password: password), as: Lock.self)
        return LockState(pinEnabled: lock.pinEnabled, idleMinutes: lock.idleMinutes)
    }

    public func saveHistory(_ enabled: Bool) async throws -> Bool {
        try await api.put(["api", "v1", "history", "preferences"], body: History(enabled: enabled), as: History.self).enabled
    }

    public func deleteHistory() async throws {
        _ = try await api.delete(["api", "v1", "history"], as: Cleared.self)
    }

    public func saveLookup(_ lookup: Lookup, _ on: Bool) async throws -> Lookups {
        try await api.put(["api", "v1", "metadata", "automation"], body: [lookup.rawValue: on], as: Lookups.self)
    }
}
