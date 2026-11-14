import Foundation

public struct TrackSources: Sendable {
    public enum Problem: Error, Equatable {
        case changed
    }

    public enum Mode: String, Decodable, Sendable {
        case automatic, manual, reviewed

        public var label: String {
            switch self {
            case .automatic: "Automatic source selection"
            case .manual: "Manual source selection"
            case .reviewed: "Reviewed sources"
            }
        }
    }

    public struct Entry: Equatable, Identifiable, Sendable {
        public let id: String
        public let primary: String
        public let secondary: String
        public let ready: Bool
        public let state: String
        public let selectable: Bool
        public let selected: Bool
        public let pinned: Bool
        public let bestGuess: Bool
        public let local: Bool
    }

    public struct Choice: Equatable, Sendable {
        public let mode: Mode
        public let revision: Int
        public let entries: [Entry]

        public var modeLabel: String { mode.label }
    }

    struct Wire: Decodable {
        struct Item: Decodable {
            let selectable: Bool
            let selected: Bool
            let source: Source
        }

        struct Source: Decodable {
            let id: String
            let provider: String
            let label: String
            let providerLabel: String?
            let ready: Bool
            let matchKind: String?
            let deviceId: String?

            enum CodingKeys: String, CodingKey {
                case id, provider, label, ready
                case providerLabel = "provider_label"
                case matchKind = "match_kind"
                case deviceId = "device_id"
            }
        }

        let mode: Mode
        let revision: Int
        let sources: [Item]
    }

    struct Choose: Encodable {
        let requestId: String
        let sourceId: String

        enum CodingKeys: String, CodingKey {
            case requestId = "request_id"
            case sourceId = "source_id"
        }
    }

    struct Reset: Encodable {
        let requestId: String
        let expectedRevision: Int

        enum CodingKeys: String, CodingKey {
            case requestId = "request_id"
            case expectedRevision = "expected_revision"
        }
    }

    struct Receipt: Decodable { let revision: Int }

    let api: HitoAPI

    public init(api: HitoAPI) {
        self.api = api
    }

    public func load(_ recording: String) async throws -> Choice {
        let wire = try await api.get(["api", "v1", "recordings", recording, "source-choice"], as: Wire.self)
        return Choice(mode: wire.mode, revision: wire.revision, entries: wire.sources.map { item in
            let source = item.source
            let local = source.provider == "local"
            let service = source.providerLabel?.trimmingCharacters(in: .whitespaces)
            let primary = local ? source.label : (service?.isEmpty == false ? service! : source.label)
            let secondary = local ? "Music folder" : (service?.isEmpty == false ? source.label.replacingOccurrences(of: " / ", with: " · ") : "Music service")
            let state = source.deviceId != nil ? "On this device" : source.ready ? "Ready" : "Not prepared"
            return Entry(id: source.id, primary: primary, secondary: secondary, ready: source.ready, state: state, selectable: item.selectable, selected: item.selected, pinned: wire.mode == .manual && item.selected, bestGuess: source.matchKind == "best_guess", local: local)
        })
    }

    public func choose(_ recording: String, source: String) async throws -> Int {
        try await changing {
            try await api.post(["api", "v1", "recordings", recording, "source-choice"], body: Choose(requestId: UUID().uuidString.lowercased(), sourceId: source), as: Receipt.self).revision
        }
    }

    public func reset(_ recording: String, revision: Int) async throws -> Int {
        try await changing {
            try await api.post(["api", "v1", "recordings", recording, "source-choice", "reset"], body: Reset(requestId: UUID().uuidString.lowercased(), expectedRevision: revision), as: Receipt.self).revision
        }
    }

    private func changing(_ work: () async throws -> Int) async throws -> Int {
        do {
            return try await work()
        } catch HitoAPI.Problem.refused(409) {
            throw Problem.changed
        }
    }
}
