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
    public var signsIn = false

    public var summary: String {
        PluginState.rows.filter { capabilities.contains($0.capability) }.map(\.name).joined(separator: " · ")
    }
}

public struct PluginResult: Decodable, Equatable, Identifiable, Sendable {
    public let id: String
    public let title: String
    public let artist: String
    public let durationMs: Int?

    public init(id: String, title: String, artist: String, durationMs: Int?) {
        self.id = id
        self.title = title
        self.artist = artist
        self.durationMs = durationMs
    }

    enum CodingKeys: String, CodingKey {
        case id, title, artist
        case durationMs = "duration_ms"
    }
}

public enum PluginAccount: Equatable, Sendable {
    case notConnected, connected, needsSignInAgain
}

public struct PluginPower: Equatable, Identifiable, Sendable {
    public let name: String
    public let providers: [String]

    public var id: String { name }
}

public struct PluginOffer: Equatable, Hashable, Identifiable, Sendable {
    public let id: String
    public let name: String
    public let revision: Int
}

public struct PluginPreview: Equatable, Sendable {
    public let digest: String
    public let name: String
    public let turnsOn: [String]
    public let signsIn: Bool
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
    struct Authorization: Decodable { let kind: String }

    struct Manifest: Decodable {
        let name: String
        let capabilities: [String]
        let authorization: Authorization?
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
                checkedAt: checkedAt.flatMap { $0 > 0 ? Date(timeIntervalSince1970: TimeInterval($0)) : nil },
                signsIn: manifest.authorization != nil
            )
        }
    }

    public enum CheckResult: Equatable, Sendable {
        case updated(PluginConnection)
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
        let revision: Int
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
            available: settings.services.filter { !connected.contains($0.id) }.map { PluginOffer(id: $0.id, name: $0.name, revision: $0.revision) }
        )
    }

    public func check(_ id: String, revision: Int, had: [String] = [], signsIn: Bool = false, accept digest: String? = nil) async throws -> CheckResult {
        let result = try await api.post(["api", "v1", "plugins", id, "refresh"], body: Refresh(revision: revision, acceptManifestDigest: digest), as: Refreshed.self)
        guard result.kind == "review_required", let digest = result.digest, let manifest = result.manifest else {
            guard let installation = result.installation else { throw HitoAPI.Problem.unexpected(200) }
            return .updated(installation.connection)
        }
        let wants = PluginState.rows.filter { manifest.capabilities.contains($0.capability) && !had.contains($0.capability) }.map(\.name)
            + (manifest.authorization != nil && !signsIn ? ["Account sign-in"] : [])
        return .review(digest: digest, wants: wants)
    }

    struct Revision: Encodable { let revision: Int }

    public func remove(_ id: String, revision: Int) async throws {
        _ = try await api.delete(["api", "v1", "plugins", id], body: Revision(revision: revision), as: Nothing.self)
    }

    struct Search: Encodable {
        let installationRevision: Int
        let query: String
        let limit: Int

        enum CodingKeys: String, CodingKey {
            case query, limit
            case installationRevision = "installation_revision"
        }
    }

    struct Found: Decodable { let items: [PluginResult] }

    struct Adopt: Encodable {
        let installationRevision: Int
        let providerRecordingId: String

        enum CodingKeys: String, CodingKey {
            case installationRevision = "installation_revision"
            case providerRecordingId = "provider_recording_id"
        }
    }

    public func search(_ id: String, revision: Int, query: String) async throws -> [PluginResult] {
        try await api.post(["api", "v1", "plugins", id, "search"], body: Search(installationRevision: revision, query: query, limit: 10), as: Found.self).items
    }

    public func adopt(_ id: String, revision: Int, result: String) async throws -> Track {
        try await api.post(["api", "v1", "plugins", id, "recordings"], body: Adopt(installationRevision: revision, providerRecordingId: result), as: Track.self)
    }

    struct AccountStatus: Decodable { let status: String }

    public func account(_ id: String) async throws -> PluginAccount {
        switch try await api.get(["api", "v1", "plugins", id, "connection"], as: AccountStatus.self).status {
        case "connected": .connected
        case "needs_reauthorization": .needsSignInAgain
        default: .notConnected
        }
    }

    public func signOut(_ id: String) async throws {
        _ = try await api.delete(["api", "v1", "plugins", id, "connection"], as: Nothing.self)
    }

    struct Install: Encodable {
        let serviceId: String
        let serviceRevision: Int
        let bearerToken: String?
        let acceptManifestDigest: String?

        enum CodingKeys: String, CodingKey {
            case serviceId = "service_id"
            case serviceRevision = "service_revision"
            case bearerToken = "bearer_token"
            case acceptManifestDigest = "accept_manifest_digest"
        }

        func encode(to encoder: Encoder) throws {
            var container = encoder.container(keyedBy: CodingKeys.self)
            try container.encode(serviceId, forKey: .serviceId)
            try container.encode(serviceRevision, forKey: .serviceRevision)
            try container.encode(bearerToken, forKey: .bearerToken)
            if let acceptManifestDigest { try container.encode(acceptManifestDigest, forKey: .acceptManifestDigest) }
        }
    }

    struct Previewed: Decodable {
        let digest: String
        let manifest: Manifest
    }

    static func key(_ token: String) -> String? {
        let trimmed = token.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? nil : trimmed
    }

    public func preview(_ offer: PluginOffer, token: String) async throws -> PluginPreview {
        let previewed = try await api.post(["api", "v1", "plugins", "preview"], body: Install(serviceId: offer.id, serviceRevision: offer.revision, bearerToken: Self.key(token), acceptManifestDigest: nil), as: Previewed.self)
        return PluginPreview(
            digest: previewed.digest,
            name: previewed.manifest.name,
            turnsOn: PluginState.rows.filter { previewed.manifest.capabilities.contains($0.capability) }.map(\.name),
            signsIn: previewed.manifest.authorization != nil
        )
    }

    public func connect(_ offer: PluginOffer, token: String, digest: String) async throws -> PluginConnection {
        try await api.post(["api", "v1", "plugins"], body: Install(serviceId: offer.id, serviceRevision: offer.revision, bearerToken: Self.key(token), acceptManifestDigest: digest), as: Installation.self).connection
    }
}
