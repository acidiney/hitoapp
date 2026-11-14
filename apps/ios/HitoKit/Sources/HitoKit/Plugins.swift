import Foundation

public struct PluginConnection: Equatable, Identifiable, Sendable {
    public enum Health: String, Decodable, Sendable {
        case ok, degraded, unreachable
        case credentialsRequired = "credentials_required"
    }

    public let id: String
    public let name: String
    public let capabilities: [String]
    public let health: Health

    public var summary: String {
        PluginState.rows.filter { capabilities.contains($0.capability) }.map(\.name).joined(separator: " · ")
    }
}

public struct PluginPower: Equatable, Identifiable, Sendable {
    public let name: String
    public let providers: [String]

    public var id: String { name }
}

public struct PluginOffer: Equatable, Identifiable, Sendable {
    public let id: String
    public let name: String
}

public struct PluginState: Equatable, Sendable {
    static let rows: [(capability: String, name: String)] = [
        ("search", "Search"),
        ("sources", "Playback"),
        ("metadata", "Track details"),
        ("playlists", "Playlists"),
        ("artwork", "Covers"),
        ("lyrics", "Lyrics"),
    ]

    public let connections: [PluginConnection]
    public let available: [PluginOffer]

    public var powers: [PluginPower] {
        Self.rows.map { row in
            PluginPower(name: row.name, providers: connections.filter { $0.capabilities.contains(row.capability) }.map(\.name))
        }
    }
}

public struct Plugins: Sendable {
    struct Manifest: Decodable {
        let name: String
        let capabilities: [String]
    }

    struct Health: Decodable { let state: PluginConnection.Health }

    struct Installation: Decodable {
        let id: String
        let serviceId: String
        let manifest: Manifest
        let health: Health?

        enum CodingKeys: String, CodingKey {
            case id, manifest, health
            case serviceId = "service_id"
        }
    }

    struct Service: Decodable {
        let id: String
        let name: String
    }

    struct Settings: Decodable {
        let installations: [Installation]
        let services: [Service]
    }

    let api: HitoAPI

    public init(api: HitoAPI) {
        self.api = api
    }

    public func load() async throws -> PluginState {
        let settings = try await api.get(["api", "v1", "plugins"], as: Settings.self)
        let connected = Set(settings.installations.map(\.serviceId))
        return PluginState(
            connections: settings.installations.map {
                PluginConnection(id: $0.id, name: $0.manifest.name, capabilities: $0.manifest.capabilities, health: $0.health?.state ?? .ok)
            },
            available: settings.services.filter { !connected.contains($0.id) }.map { PluginOffer(id: $0.id, name: $0.name) }
        )
    }
}
