import Foundation

public struct PluginConnection: Equatable, Hashable, Identifiable, Sendable {
    public enum Health: String, Decodable, Sendable {
        case ok, degraded, unreachable
        case credentialsRequired = "credentials_required"
    }

    public let id: String
    public let name: String
    public let capabilities: [String]
    public let health: Health
    public let revision: Int
    public let checkedAt: Date?

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
        let revision: Int
        let checkedAt: Int?

        enum CodingKeys: String, CodingKey {
            case id, manifest, health, revision
            case serviceId = "service_id"
            case checkedAt = "checked_at"
        }

        var connection: PluginConnection {
            PluginConnection(
                id: id,
                name: manifest.name,
                capabilities: manifest.capabilities,
                health: health?.state ?? .ok,
                revision: revision,
                checkedAt: checkedAt.flatMap { $0 > 0 ? Date(timeIntervalSince1970: TimeInterval($0)) : nil }
            )
        }
    }

    public enum CheckResult: Equatable, Sendable {
        case updated(revision: Int)
        case review(digest: String, wants: [String])
    }

    struct Refresh: Encodable {
        struct Keep: Encodable { let action = "keep" }

        let revision: Int
        let acceptManifestDigest: String?
        let credential = Keep()

        enum CodingKeys: String, CodingKey {
            case revision, credential
            case acceptManifestDigest = "accept_manifest_digest"
        }

        func encode(to encoder: Encoder) throws {
            var container = encoder.container(keyedBy: CodingKeys.self)
            try container.encode(revision, forKey: .revision)
            try container.encode(credential, forKey: .credential)
            try container.encode(acceptManifestDigest, forKey: .acceptManifestDigest)
        }
    }

    struct Refreshed: Decodable {
        let kind: String
        let digest: String?
        let manifest: Manifest?
        let installation: Installation?
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
            connections: settings.installations.map(\.connection),
            available: settings.services.filter { !connected.contains($0.id) }.map { PluginOffer(id: $0.id, name: $0.name) }
        )
    }

    public func check(_ id: String, revision: Int, had: [String] = [], accept digest: String? = nil) async throws -> CheckResult {
        let result = try await api.post(["api", "v1", "plugins", id, "refresh"], body: Refresh(revision: revision, acceptManifestDigest: digest), as: Refreshed.self)
        guard result.kind == "review_required", let digest = result.digest, let manifest = result.manifest else {
            return .updated(revision: result.installation?.revision ?? revision)
        }
        let wants = PluginState.rows.filter { manifest.capabilities.contains($0.capability) && !had.contains($0.capability) }.map(\.name)
        return .review(digest: digest, wants: wants)
    }

    public func remove(_ id: String) async throws {
        _ = try await api.delete(["api", "v1", "plugins", id], as: Nothing.self)
    }
}
