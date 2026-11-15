import Foundation

public struct LyricChoices: Sendable {
    public enum Selector: Codable, Equatable, Sendable {
        case observation(String)
        case identity(String, revision: Int)

        enum CodingKeys: String, CodingKey {
            case kind
            case observationId = "observation_id"
            case identityId = "identity_id"
            case identityRevision = "identity_revision"
        }

        public init(from decoder: Decoder) throws {
            let values = try decoder.container(keyedBy: CodingKeys.self)
            if try values.decode(String.self, forKey: .kind) == "identity" {
                self = .identity(try values.decode(String.self, forKey: .identityId), revision: try values.decode(Int.self, forKey: .identityRevision))
            } else {
                self = .observation(try values.decode(String.self, forKey: .observationId))
            }
        }

        public func encode(to encoder: Encoder) throws {
            var values = encoder.container(keyedBy: CodingKeys.self)
            switch self {
            case let .observation(id):
                try values.encode("observation", forKey: .kind)
                try values.encode(id, forKey: .observationId)
            case let .identity(id, revision):
                try values.encode("identity", forKey: .kind)
                try values.encode(id, forKey: .identityId)
                try values.encode(revision, forKey: .identityRevision)
            }
        }
    }

    public struct Identity: Decodable, Equatable, Identifiable, Sendable {
        public let label: String
        public let installationRevision: Int
        public let selectable: Bool
        public let selector: Selector

        public var id: String {
            switch selector {
            case let .observation(id): "observation:\(id)"
            case let .identity(id, _): "identity:\(id)"
            }
        }

        public var removable: Bool {
            if case .identity = selector { return true }
            return false
        }

        public var origin: String { removable ? "Chosen by you" : "From this track's details" }

        public var identityId: String? {
            if case let .identity(id, _) = selector { return id }
            return nil
        }

        enum CodingKeys: String, CodingKey {
            case label, selectable, selector
            case installationRevision = "installation_revision"
        }
    }

    public struct Service: Decodable, Equatable, Identifiable, Sendable {
        public let installationId: String
        public let installationRevision: Int
        public let label: String

        public var id: String { installationId }

        enum CodingKeys: String, CodingKey {
            case label
            case installationId = "installation_id"
            case installationRevision = "installation_revision"
        }
    }

    public struct Match: Decodable, Equatable, Identifiable, Sendable {
        struct Release: Decodable, Equatable, Sendable { let title: String }

        public let id: String
        public let title: String
        public let artist: String
        public let durationMs: Int?
        let release: Release?

        public var album: String? { release?.title }

        enum CodingKeys: String, CodingKey {
            case id, title, artist, release
            case durationMs = "duration_ms"
        }
    }

    public struct Review: Equatable, Sendable {
        public let stamp: String
        public let match: Match
    }

    struct List<Item: Decodable>: Decodable { let items: [Item] }

    struct ReviewWire: Decodable {
        let reviewStamp: String
        let candidate: Match

        enum CodingKeys: String, CodingKey {
            case candidate
            case reviewStamp = "review_stamp"
        }
    }

    struct Read: Encodable {
        let installationRevision: Int
        let selector: Selector

        enum CodingKeys: String, CodingKey {
            case selector
            case installationRevision = "installation_revision"
        }
    }

    struct Search: Encodable {
        let installationId: String
        let installationRevision: Int
        let query: String

        enum CodingKeys: String, CodingKey {
            case query
            case installationId = "installation_id"
            case installationRevision = "installation_revision"
        }
    }

    struct Keep: Encodable {
        let candidate: SourceAttachment.Candidate
        let reviewStamp: String

        enum CodingKeys: String, CodingKey {
            case candidate
            case reviewStamp = "review_stamp"
        }
    }

    struct Lookup: Decodable {
        struct Document: Decodable { let lyrics: Body? }

        struct Body: Decodable {
            let lines: [Timed]?
            let content: Content?
        }

        struct Timed: Decodable {
            let startMs: Int
            let endMs: Int
            let text: String

            enum CodingKeys: String, CodingKey {
                case text
                case startMs = "start_ms"
                case endMs = "end_ms"
            }
        }

        struct Anchored: Decodable {
            let startMs: Int
            let text: String

            enum CodingKeys: String, CodingKey {
                case text
                case startMs = "start_ms"
            }
        }

        enum Content: Decodable {
            case plain([String])
            case anchored([Anchored])

            enum CodingKeys: String, CodingKey { case kind, lines }

            init(from decoder: Decoder) throws {
                let values = try decoder.container(keyedBy: CodingKeys.self)
                if try values.decode(String.self, forKey: .kind) == "anchored" {
                    self = .anchored(try values.decode([Anchored].self, forKey: .lines))
                } else {
                    self = .plain(try values.decode([String].self, forKey: .lines))
                }
            }
        }

        let status: String
        let label: String?
        let document: Document?
    }

    struct Empty: Encodable {}

    let api: HitoAPI

    public init(api: HitoAPI) {
        self.api = api
    }

    public func identities(_ recording: String) async throws -> [Identity] {
        try await api.get(["api", "v1", "recordings", recording, "lyrics", "identities"], as: List<Identity>.self).items
    }

    public func read(_ recording: String, _ identity: Identity) async throws -> LyricsResult {
        let lookup = try await api.post(["api", "v1", "recordings", recording, "lyrics", "identities", "read"], body: Read(installationRevision: identity.installationRevision, selector: identity.selector), as: Lookup.self)
        return Self.result(lookup, label: identity.label)
    }

    public func services(_ recording: String) async throws -> [Service] {
        try await api.get(["api", "v1", "recordings", recording, "lyrics", "services"], as: List<Service>.self).items
    }

    public func search(_ recording: String, _ service: Service, query: String) async throws -> [Match] {
        try await api.post(["api", "v1", "recordings", recording, "lyrics", "candidates"], body: Search(installationId: service.installationId, installationRevision: service.installationRevision, query: query), as: List<Match>.self).items
    }

    public func review(_ recording: String, _ service: Service, _ match: Match) async throws -> Review {
        let wire = try await api.post(["api", "v1", "recordings", recording, "lyrics", "identities", "review"], body: Self.candidate(service, match), as: ReviewWire.self)
        return Review(stamp: wire.reviewStamp, match: wire.candidate)
    }

    public func keep(_ recording: String, _ service: Service, _ match: Match, stamp: String) async throws -> Identity {
        try await api.post(["api", "v1", "recordings", recording, "lyrics", "identities"], body: Keep(candidate: Self.candidate(service, match), reviewStamp: stamp), as: Identity.self)
    }

    public func remove(_ recording: String, identity: String) async throws {
        _ = try await api.delete(["api", "v1", "recordings", recording, "lyrics", "identities", identity], as: Nothing.self)
    }

    public func automatic(_ recording: String) async throws -> LyricsResult {
        let lookup = try await api.post(["api", "v1", "recordings", recording, "lyrics", "automatic"], body: Empty(), as: Lookup.self)
        return Self.result(lookup, label: lookup.label ?? "")
    }

    public func best(_ recording: String, automatic: Bool) async throws -> LyricsResult {
        let found = try await identities(recording).filter(\.selectable)
        if let pick = found.first(where: \.removable) ?? found.first {
            let result = try await read(recording, pick)
            if result != .none { return result }
        }
        return automatic ? try await self.automatic(recording) : .none
    }

    static func candidate(_ service: Service, _ match: Match) -> SourceAttachment.Candidate {
        SourceAttachment.Candidate(installationId: service.installationId, installationRevision: service.installationRevision, providerRecordingId: match.id)
    }

    static func result(_ lookup: Lookup, label: String) -> LyricsResult {
        guard lookup.status.hasPrefix("ready"), let body = lookup.document?.lyrics else { return .none }
        let lines: [LyricLine]
        if let timed = body.lines {
            lines = timed.map { LyricLine(start: Double($0.startMs) / 1000, end: Double($0.endMs) / 1000, text: $0.text) }
        } else if case let .anchored(anchored) = body.content {
            lines = anchored.enumerated().map { index, line in
                let next = anchored.indices.contains(index + 1) ? Double(anchored[index + 1].startMs) / 1000 : .infinity
                return LyricLine(start: Double(line.startMs) / 1000, end: next, text: line.text)
            }
        } else if case let .plain(text) = body.content {
            lines = Self.lines(plain: text)
        } else {
            lines = []
        }
        return lines.isEmpty ? .none : .lines(lines, from: label)
    }

    public static func lines(plain: [String]) -> [LyricLine] {
        plain.map { LyricLine(start: -1, end: -1, text: $0) }
    }

    static func automaticKey(_ account: String) -> String { "app.hito.lyrics-auto.\(account)" }

    public static func automaticEnabled(defaults: UserDefaults = .standard, account: String) -> Bool {
        defaults.string(forKey: automaticKey(account)) != "off"
    }

    public static func setAutomatic(_ on: Bool, defaults: UserDefaults = .standard, account: String) {
        defaults.set(on ? "on" : "off", forKey: automaticKey(account))
    }
}
