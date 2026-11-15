import Foundation

public enum JSONValue: Codable, Equatable, Sendable {
    case null
    case bool(Bool)
    case integer(Int)
    case number(Double)
    case string(String)
    case array([JSONValue])
    case object([String: JSONValue])

    public init(from decoder: Decoder) throws {
        let container = try decoder.singleValueContainer()
        if container.decodeNil() {
            self = .null
        } else if let value = try? container.decode(Bool.self) {
            self = .bool(value)
        } else if let value = try? container.decode(Int.self) {
            self = .integer(value)
        } else if let value = try? container.decode(Double.self) {
            self = .number(value)
        } else if let value = try? container.decode(String.self) {
            self = .string(value)
        } else if let value = try? container.decode([JSONValue].self) {
            self = .array(value)
        } else {
            self = .object(try container.decode([String: JSONValue].self))
        }
    }

    public func encode(to encoder: Encoder) throws {
        var container = encoder.singleValueContainer()
        switch self {
        case .null: try container.encodeNil()
        case let .bool(value): try container.encode(value)
        case let .integer(value): try container.encode(value)
        case let .number(value): try container.encode(value)
        case let .string(value): try container.encode(value)
        case let .array(value): try container.encode(value)
        case let .object(value): try container.encode(value)
        }
    }
}

public struct MetadataProvider: Equatable, Hashable, Identifiable, Sendable {
    public let id: String
    public let name: String
    let kind: String
    let installation: String?
}

public struct ArtistCandidate: Equatable, Identifiable, Sendable {
    public let id: String
    public let name: String
    public let detail: String?

    public init(id: String, name: String, detail: String?) {
        self.id = id
        self.name = name
        self.detail = detail
    }
}

public struct ArtistProfile: Equatable, Sendable {
    public let name: String
    public let detail: String?
}

public struct ArtistReview: Sendable {
    public struct Session: Equatable, Sendable {
        let recording: String
        let context: JSONValue
    }

    public struct Problem: Error, Equatable {
        public let message: String
    }

    struct Settings: Decodable {
        struct Provider: Decodable {
            let kind: String
            let installationId: String?

            enum CodingKeys: String, CodingKey {
                case kind
                case installationId = "installation_id"
            }
        }

        let enabled: Bool
        let providers: [Provider]
    }

    struct ProviderBody: Encodable {
        let kind: String
        let installationId: String?

        enum CodingKeys: String, CodingKey {
            case kind
            case installationId = "installation_id"
        }
    }

    struct Start: Encodable { let provider: ProviderBody }

    struct Search: Encodable {
        struct Query: Encodable {
            let query: String
            let limit = 20
            let cursor: String? = nil
            let snapshot: String? = nil

            enum CodingKeys: String, CodingKey { case query, limit, cursor, snapshot }

            func encode(to encoder: Encoder) throws {
                var container = encoder.container(keyedBy: CodingKeys.self)
                try container.encode(query, forKey: .query)
                try container.encode(limit, forKey: .limit)
                try container.encode(cursor, forKey: .cursor)
                try container.encode(snapshot, forKey: .snapshot)
            }
        }

        let context: JSONValue
        let search: Query
    }

    struct Pick: Encodable {
        let context: JSONValue
        let providerArtistId: String

        enum CodingKeys: String, CodingKey {
            case context
            case providerArtistId = "provider_artist_id"
        }
    }

    struct Named: Decodable {
        let id: String
        let name: String
    }

    struct Candidate: Decodable {
        let artist: Named
        let kind: String?
        let disambiguation: String?
        let country: String?
        let beginYear: Int?

        enum CodingKeys: String, CodingKey {
            case artist, kind, disambiguation, country
            case beginYear = "begin_year"
        }
    }

    struct Found: Decodable {
        struct Page: Decodable { let items: [Candidate] }

        let status: String
        let context: JSONValue?
        let page: Page?
    }

    struct Profile: Decodable {
        let artist: Named
        let kind: String?
        let disambiguation: String?
    }

    struct Previewed: Decodable {
        let status: String
        let context: JSONValue?
        let profile: Profile?
    }

    struct Confirmed: Decodable { let id: String }

    let api: HitoAPI

    public init(api: HitoAPI) {
        self.api = api
    }

    public func providers(named names: [String: String]) async throws -> [MetadataProvider] {
        let settings = try await api.get(["api", "v1", "metadata", "settings"], as: Settings.self)
        guard settings.enabled else { return [] }
        return settings.providers.compactMap { provider in
            switch provider.kind {
            case "builtin_musicbrainz":
                MetadataProvider(id: provider.kind, name: "MusicBrainz", kind: provider.kind, installation: nil)
            case "http_plugin":
                provider.installationId.map { MetadataProvider(id: $0, name: names[$0] ?? "Plugin", kind: provider.kind, installation: $0) }
            default:
                nil
            }
        }
    }

    public func start(recording: String, provider: MetadataProvider) async throws -> Session {
        let context = try await guarded {
            try await api.post(path(recording, "context"), body: Start(provider: ProviderBody(kind: provider.kind, installationId: provider.installation)), as: JSONValue.self)
        }
        return Session(recording: recording, context: context)
    }

    public func search(_ query: String, in session: Session) async throws -> (Session, [ArtistCandidate]) {
        let found = try await guarded {
            try await api.post(path(session.recording, "search"), body: Search(context: session.context, search: .init(query: query.trimmingCharacters(in: .whitespacesAndNewlines))), as: Found.self)
        }
        guard found.status == "available", let page = found.page else { throw Self.problem(found.status) }
        let candidates = page.items.map {
            ArtistCandidate(id: $0.artist.id, name: $0.artist.name, detail: Self.facts([Self.kind($0.kind), $0.disambiguation, $0.country, $0.beginYear.map(String.init)]))
        }
        return (Session(recording: session.recording, context: found.context ?? session.context), candidates)
    }

    public func preview(_ candidate: ArtistCandidate, in session: Session) async throws -> (Session, ArtistProfile) {
        let previewed = try await guarded {
            try await api.post(path(session.recording, "preview"), body: Pick(context: session.context, providerArtistId: candidate.id), as: Previewed.self)
        }
        guard previewed.status == "available", let profile = previewed.profile else { throw Self.problem(previewed.status) }
        return (Session(recording: session.recording, context: previewed.context ?? session.context), ArtistProfile(name: profile.artist.name, detail: Self.facts([Self.kind(profile.kind), profile.disambiguation])))
    }

    public func confirm(_ candidate: ArtistCandidate, in session: Session) async throws {
        _ = try await guarded {
            try await api.post(path(session.recording, "confirm"), body: Pick(context: session.context, providerArtistId: candidate.id), as: Confirmed.self)
        }
    }

    private func path(_ recording: String, _ step: String) -> [String] {
        ["api", "v1", "recordings", recording, "artist-review", step]
    }

    private func guarded<Value>(_ call: () async throws -> Value) async throws -> Value {
        do {
            return try await call()
        } catch HitoAPI.Problem.refused(409) {
            throw Problem(message: "This track changed while you were choosing. Start again.")
        }
    }

    static func problem(_ status: String) -> Problem {
        switch status {
        case "disabled": Problem(message: "Track details from plugins are off in your settings.")
        case "unsupported": Problem(message: "This provider can't search for artists.")
        case "cancelled": Problem(message: "The search was cancelled. Try again.")
        default: Problem(message: "The provider didn't answer. Try again.")
        }
    }

    static func kind(_ kind: String?) -> String? {
        guard let kind, let first = kind.first else { return nil }
        return first.uppercased() + kind.dropFirst()
    }

    static func facts(_ parts: [String?]) -> String? {
        let present = parts.compactMap { $0 }.filter { !$0.isEmpty }
        return present.isEmpty ? nil : present.joined(separator: " · ")
    }
}
