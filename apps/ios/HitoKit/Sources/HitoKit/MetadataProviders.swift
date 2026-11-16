import Foundation

public struct MetadataProviders: Sendable {
    public enum Problem: Error, Equatable {
        case changed
    }

    public enum Provider: Codable, Equatable, Hashable, Sendable {
        case musicbrainz
        case plugin(String)

        enum CodingKeys: String, CodingKey {
            case kind
            case installationId = "installation_id"
        }

        public init(from decoder: Decoder) throws {
            let values = try decoder.container(keyedBy: CodingKeys.self)
            if try values.decode(String.self, forKey: .kind) == "http_plugin" {
                self = .plugin(try values.decode(String.self, forKey: .installationId))
            } else {
                self = .musicbrainz
            }
        }

        public func encode(to encoder: Encoder) throws {
            var values = encoder.container(keyedBy: CodingKeys.self)
            switch self {
            case .musicbrainz:
                try values.encode("builtin_musicbrainz", forKey: .kind)
            case let .plugin(id):
                try values.encode("http_plugin", forKey: .kind)
                try values.encode(id, forKey: .installationId)
            }
        }

        public func label(_ connections: [PluginConnection]) -> String {
            switch self {
            case .musicbrainz: "MusicBrainz"
            case let .plugin(id): connections.first { $0.id == id && $0.capabilities.contains("catalog") }?.name ?? "Unavailable saved provider"
            }
        }

        public func detail(_ connections: [PluginConnection]) -> String {
            switch self {
            case .musicbrainz: "Built in"
            case let .plugin(id): connections.contains { $0.id == id && $0.capabilities.contains("catalog") } ? "Plugin · connected" : "Plugin · not connected"
            }
        }
    }

    public struct Preferences: Codable, Equatable, Sendable {
        public static let most = 8

        public let revision: Int
        public var enabled: Bool
        public var providers: [Provider]

        public init(revision: Int, enabled: Bool, providers: [Provider]) {
            self.revision = revision
            self.enabled = enabled
            self.providers = providers
        }

        public var canAdd: Bool { providers.count < Self.most }

        public func available(_ connections: [PluginConnection]) -> [Provider] {
            let options = [Provider.musicbrainz] + connections.filter { $0.capabilities.contains("catalog") }.map { Provider.plugin($0.id) }
            return options.filter { !providers.contains($0) }
        }

        public mutating func move(_ provider: Provider, by offset: Int) {
            guard let index = providers.firstIndex(of: provider) else { return }
            let target = index + offset
            guard providers.indices.contains(target) else { return }
            providers.swapAt(index, target)
        }
    }

    let api: HitoAPI

    public init(api: HitoAPI) {
        self.api = api
    }

    public func load() async throws -> Preferences {
        try await api.get(["api", "v1", "metadata", "settings"], as: Preferences.self)
    }

    public func save(_ preferences: Preferences) async throws -> Preferences {
        do {
            return try await api.put(["api", "v1", "metadata", "settings"], body: preferences, as: Preferences.self)
        } catch HitoAPI.Problem.refused(409) {
            throw Problem.changed
        }
    }
}
