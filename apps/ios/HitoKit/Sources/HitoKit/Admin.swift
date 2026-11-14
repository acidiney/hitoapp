import Foundation

public struct ServerAccount: Equatable, Identifiable, Sendable {
    public let id: String
    public let username: String
    public let displayName: String?
    public let admin: Bool
    public let lastActive: Date?

    public var name: String { displayName ?? username }

    public func hint(now: Date = Date()) -> String {
        let role = admin ? "Administrator" : "Member"
        let activity: String
        if let lastActive, now.timeIntervalSince(lastActive) < 60 {
            activity = "active now"
        } else if let lastActive {
            let formatter = RelativeDateTimeFormatter()
            formatter.locale = Locale(identifier: "en_US_POSIX")
            formatter.unitsStyle = .full
            activity = "active \(formatter.localizedString(for: lastActive, relativeTo: now))"
        } else {
            activity = "no open sessions"
        }
        let identity = displayName == nil ? "" : "\(username) · "
        return "\(identity)\(role) · \(activity)"
    }
}

public struct ServerLibrary: Equatable, Identifiable, Sendable {
    public let id: String
    public let name: String
    public let personal: Bool
}

public struct MusicFolder: Equatable, Identifiable, Sendable {
    public let id: String
    public let name: String
    public let revision: Int
    public let available: Bool
    public let scanning: Int?

    public var status: String {
        let reach = available ? "read-only" : "can't be reached"
        guard let scanning else { return reach }
        return "\(reach) · scanning \(scanning)%"
    }
}

public struct DirectoryPlace: Equatable, Hashable, Sendable {
    let directoryId: String
    let components: [String]
}

public struct ServerDirectory: Equatable, Identifiable, Sendable {
    public let name: String
    public let place: DirectoryPlace

    public var id: String { ([place.directoryId] + place.components).joined(separator: "/") }
}

public struct Admin: Sendable {
    struct Account: Decodable {
        let id: String
        let username: String
        let displayName: String?
        let admin: Bool
        let lastActiveAt: Int?

        enum CodingKeys: String, CodingKey {
            case id, username, admin
            case displayName = "display_name"
            case lastActiveAt = "last_active_at"
        }
    }

    struct Credentials: Encodable {
        let username: String
        let password: String
    }

    struct Confirm: Encodable { let username: String }

    struct Revision: Encodable { let revision: Int }

    struct Library: Decodable {
        let id: String
        let name: String
        let personal: Bool?
    }

    struct Job: Decodable {
        let state: String
        let progress: Int
    }

    struct Folder: Decodable {
        let id: String
        let name: String
        let revision: Int
        let available: Bool
        let latestJob: Job?

        enum CodingKeys: String, CodingKey {
            case id, name, revision, available
            case latestJob = "latest_job"
        }
    }

    struct Removal: Decodable {
        let removed: Int
        let keptReferenced: Int
        let more: Bool

        enum CodingKeys: String, CodingKey {
            case removed, more
            case keptReferenced = "kept_referenced"
        }
    }

    struct Ref: Codable {
        let directoryId: String
        let components: [String]?

        enum CodingKeys: String, CodingKey {
            case components
            case directoryId = "directory_id"
        }

        var place: DirectoryPlace { DirectoryPlace(directoryId: directoryId, components: components ?? []) }
    }

    struct Shared: Decodable {
        let id: String
        let name: String
        let available: Bool
    }

    struct Browse: Encodable { let directory: Ref }

    struct Entry: Decodable {
        let name: String
        let directory: Ref
    }

    struct Page: Decodable { let directories: [Entry] }

    struct Connect: Encodable { let directory: Ref }

    let api: HitoAPI

    public init(api: HitoAPI) {
        self.api = api
    }

    public func accounts() async throws -> [ServerAccount] {
        try await api.get(["api", "v1", "users"], as: [Account].self).map {
            ServerAccount(id: $0.id, username: $0.username, displayName: $0.displayName, admin: $0.admin, lastActive: $0.lastActiveAt.map { Date(timeIntervalSince1970: TimeInterval($0)) })
        }
    }

    public func createAccount(username: String, password: String) async throws {
        _ = try await api.post(["api", "v1", "users"], body: Credentials(username: username.trimmingCharacters(in: .whitespacesAndNewlines), password: password), as: Nothing.self)
    }

    public func deleteAccount(_ id: String, username: String) async throws {
        _ = try await api.delete(["api", "v1", "users", id], body: Confirm(username: username), as: Nothing.self)
    }

    public func setAccess(library: String, account: String, to granted: Bool) async throws {
        let path = ["api", "v1", "libraries", library, "members", account]
        if granted {
            _ = try await api.put(path, body: Optional<String>.none, as: Nothing.self)
        } else {
            _ = try await api.delete(path, as: Nothing.self)
        }
    }

    public func libraries() async throws -> [ServerLibrary] {
        try await api.get(["api", "v1", "libraries"], as: [Library].self).map { ServerLibrary(id: $0.id, name: $0.name, personal: $0.personal ?? false) }
    }

    public func folders(_ library: String) async throws -> [MusicFolder] {
        try await api.get(["api", "v1", "libraries", library, "folders"], as: [Folder].self).map { folder in
            let running = folder.latestJob.flatMap { ["queued", "running"].contains($0.state) ? $0.progress : nil }
            return MusicFolder(id: folder.id, name: folder.name, revision: folder.revision, available: folder.available, scanning: running)
        }
    }

    public func scan(folder: String) async throws {
        _ = try await api.post(["api", "v1", "folders", folder, "scan"], body: Optional<String>.none, as: Nothing.self)
    }

    public func scanAll(_ library: String) async throws {
        _ = try await api.post(["api", "v1", "libraries", library, "scan"], body: Optional<String>.none, as: Nothing.self)
    }

    public func removeFolder(_ id: String, revision: Int) async throws {
        _ = try await api.delete(["api", "v1", "folders", id], body: Revision(revision: revision), as: Nothing.self)
    }

    public func removeMissing(_ library: String) async throws -> String {
        let result = try await api.delete(["api", "v1", "libraries", library, "missing-recordings"], as: Removal.self)
        if result.removed == 0 && result.keptReferenced == 0 { return "No tracks with missing files." }
        var parts = ["Removed \(result.removed) \(result.removed == 1 ? "track" : "tracks")."]
        if result.keptReferenced == 1 {
            parts.append("1 stays because it's in a playlist, liked or downloaded.")
        } else if result.keptReferenced > 1 {
            parts.append("\(result.keptReferenced) stay because they're in playlists, liked or downloaded.")
        }
        if result.more { parts.append("More are left. Run it again.") }
        return parts.joined(separator: " ")
    }

    public func sharedDirectories() async throws -> [ServerDirectory] {
        try await api.get(["api", "v1", "storage", "directories"], as: [Shared].self)
            .filter(\.available)
            .map { ServerDirectory(name: $0.name, place: DirectoryPlace(directoryId: $0.id, components: [])) }
    }

    public func browse(_ place: DirectoryPlace) async throws -> [ServerDirectory] {
        let page = try await api.post(["api", "v1", "storage", "browse"], body: Browse(directory: Ref(directoryId: place.directoryId, components: place.components)), as: Page.self)
        return page.directories.map { ServerDirectory(name: $0.name, place: $0.directory.place) }
    }

    public func addFolder(_ place: DirectoryPlace, to library: String) async throws {
        _ = try await api.post(["api", "v1", "libraries", library, "folders"], body: Connect(directory: Ref(directoryId: place.directoryId, components: place.components)), as: Nothing.self)
    }

    struct CorrectionPage: Decodable {
        let items: [SharedCorrection]
        let total: Int
        let offset: Int
        let limit: Int
    }

    struct Undo: Encodable {
        let requestId: UUID
        let expectedRevision: Int

        enum CodingKeys: String, CodingKey {
            case requestId = "request_id"
            case expectedRevision = "expected_revision"
        }
    }

    struct UndoReceipt: Decodable {
        let applied: Bool
    }

    public func sharedCorrections(offset: Int) async throws -> SharedCorrectionPage {
        let page = try await api.get(["api", "v1", "artist-corrections"], query: [URLQueryItem(name: "offset", value: String(offset)), URLQueryItem(name: "limit", value: "25")], as: CorrectionPage.self)
        return SharedCorrectionPage(items: page.items, total: page.total, offset: page.offset, limit: page.limit)
    }

    public func toggle(_ correction: SharedCorrection, request: UUID) async throws -> String {
        let receipt = try await api.post(["api", "v1", "artist-corrections", correction.id, "undo"], body: Undo(requestId: request, expectedRevision: correction.revision), as: UndoReceipt.self)
        return receipt.applied ? "The shared correction was restored." : "The shared correction was withdrawn."
    }
}

public struct SharedCorrectionPage: Equatable, Sendable {
    public let items: [SharedCorrection]
    public let total: Int
    public let offset: Int
    public let limit: Int
}

public struct SharedCorrection: Decodable, Equatable, Identifiable, Sendable {
    public enum Kind: String, Decodable, Sendable {
        case unlink
        case split
    }

    public let id: String
    public let kind: Kind
    public let recordings: Int
    public let retired: Int
    public let applied: Bool
    public let revision: Int

    public init(id: String, kind: Kind, recordings: Int, retired: Int, applied: Bool, revision: Int) {
        self.id = id
        self.kind = kind
        self.recordings = recordings
        self.retired = retired
        self.applied = applied
        self.revision = revision
    }

    public var title: String {
        "\(kind == .unlink ? "Unlink" : "Split") · \(recordings) \(recordings == 1 ? "recording" : "recordings")"
    }

    public var detail: String? {
        retired > 0 ? "\(retired) no longer \(retired == 1 ? "matches" : "match") the file tags" : nil
    }

    public var status: String { applied ? "Applied" : "Withdrawn" }
}

