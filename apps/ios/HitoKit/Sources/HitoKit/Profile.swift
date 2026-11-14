import Foundation

public struct Device: Equatable, Identifiable, Sendable {
    public let id: String
    public let name: String
    public let current: Bool
    public let lastSeen: Date
}

public struct Profile: Sendable {
    struct Session: Decodable {
        let id: String
        let client: String?
        let lastSeenAt: Int
        let current: Bool

        enum CodingKeys: String, CodingKey {
            case id, client, current
            case lastSeenAt = "last_seen_at"
        }
    }

    struct Rename: Encodable {
        let displayName: String?

        enum CodingKeys: String, CodingKey { case displayName = "display_name" }

        func encode(to encoder: Encoder) throws {
            var container = encoder.container(keyedBy: CodingKeys.self)
            try container.encode(displayName, forKey: .displayName)
        }
    }

    let api: HitoAPI

    public init(api: HitoAPI) {
        self.api = api
    }

    public func devices() async throws -> [Device] {
        try await api.get(["api", "v1", "account", "sessions"], as: [Session].self)
            .sorted { ($0.current ? 1 : 0, $0.lastSeenAt) > ($1.current ? 1 : 0, $1.lastSeenAt) }
            .map { Device(id: $0.id, name: $0.client ?? "Unknown device", current: $0.current, lastSeen: Date(timeIntervalSince1970: TimeInterval($0.lastSeenAt))) }
    }

    public func signOut(device id: String) async throws {
        _ = try await api.delete(["api", "v1", "account", "sessions", id], as: Nothing.self)
    }

    public enum PhotoProblem: Error, Equatable {
        case tooBig
        case notAPicture

        public var message: String {
            switch self {
            case .tooBig: "That photo is over 2 MB. Choose a smaller one."
            case .notAPicture: "Hito can use a JPEG, PNG or WebP photo. Choose another one."
            }
        }
    }

    public static func photoPath(account id: String) -> [String] {
        ["api", "v1", "accounts", id, "avatar"]
    }

    public func setPhoto(_ bytes: Data) async throws -> User {
        do {
            return try await api.upload(["api", "v1", "account", "avatar"], bytes: bytes, as: User.self)
        } catch HitoAPI.Problem.refused(413) {
            throw PhotoProblem.tooBig
        } catch HitoAPI.Problem.refused(422) {
            throw PhotoProblem.notAPicture
        }
    }

    public func removePhoto() async throws -> User {
        try await api.delete(["api", "v1", "account", "avatar"], as: User.self)
    }

    public func rename(_ name: String) async throws -> User {
        let trimmed = name.trimmingCharacters(in: .whitespacesAndNewlines)
        return try await api.put(["api", "v1", "account", "display-name"], body: Rename(displayName: trimmed.isEmpty ? nil : trimmed), as: User.self)
    }
}

public struct SourceOrder: Sendable {
    public enum Entry: Equatable, Hashable, Sendable {
        case localFiles
        case savedCopies
        case provider(String)
    }

    public struct Order: Equatable, Sendable {
        public let revision: Int
        public var fallback: Bool
        public var entries: [Entry]

        public mutating func move(_ index: Int, by offset: Int) {
            let target = index + offset
            guard entries.indices.contains(index), entries.indices.contains(target) else { return }
            entries.swapAt(index, target)
        }
    }

    struct Wire: Codable {
        struct Item: Codable {
            let kind: String
            let installationId: String?

            enum CodingKeys: String, CodingKey {
                case kind
                case installationId = "installation_id"
            }
        }

        let revision: Int
        let fallback: Bool
        let entries: [Item]
    }

    let api: HitoAPI

    public init(api: HitoAPI) {
        self.api = api
    }

    public func load() async throws -> Order {
        Self.order(try await api.get(["api", "v1", "source-preferences"], as: Wire.self))
    }

    public func save(_ order: Order) async throws -> Order {
        let items = order.entries.map { entry -> Wire.Item in
            switch entry {
            case .localFiles: Wire.Item(kind: "local_files", installationId: nil)
            case .savedCopies: Wire.Item(kind: "saved_copies", installationId: nil)
            case .provider(let id): Wire.Item(kind: "provider", installationId: id)
            }
        }
        return Self.order(try await api.put(["api", "v1", "source-preferences"], body: Wire(revision: order.revision, fallback: order.fallback, entries: items), as: Wire.self))
    }

    static func order(_ wire: Wire) -> Order {
        Order(revision: wire.revision, fallback: wire.fallback, entries: wire.entries.compactMap { item in
            switch item.kind {
            case "local_files": .localFiles
            case "saved_copies": .savedCopies
            case "provider": item.installationId.map(Entry.provider)
            default: nil
            }
        })
    }
}

public enum PasswordChange {
    public enum Problem: Error, Equatable {
        case wrongPassword
        case tooManyTries
    }

    static let minimum = 12

    public static func problem(new: String, again: String) -> String? {
        if new.count < minimum { return "Use a new password of at least \(minimum) characters." }
        if new != again { return "The new passwords don't match." }
        return nil
    }
}

extension Profile {
    struct ChangePassword: Encodable {
        let currentPassword: String
        let newPassword: String

        enum CodingKeys: String, CodingKey {
            case currentPassword = "current_password"
            case newPassword = "new_password"
        }
    }

    struct Revoked: Decodable { let revoked: Int }

    public func changePassword(current: String, new: String) async throws -> String {
        let result: Revoked
        do {
            result = try await api.post(["api", "v1", "account", "password"], body: ChangePassword(currentPassword: current, newPassword: new), as: Revoked.self)
        } catch HitoAPI.Problem.refused(422) {
            throw PasswordChange.Problem.wrongPassword
        } catch HitoAPI.Problem.refused(429) {
            throw PasswordChange.Problem.tooManyTries
        }
        if result.revoked == 0 { return "Password changed." }
        return "Password changed. \(result.revoked) other \(result.revoked == 1 ? "device was" : "devices were") signed out."
    }
}
