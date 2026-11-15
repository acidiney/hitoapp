import Foundation

public struct PluginSignIn: Sendable {
    public struct Attempt: Equatable, Sendable {
        public let id: String
        public let url: URL
        public let expires: Date
    }

    public struct Refused: Error, Equatable {
        public let reason: String?
    }

    public enum State: Equatable, Sendable {
        case waiting, completed, failed, gone
    }

    struct Revision: Encodable { let revision: Int }

    struct Begun: Decodable {
        let id: String
        let authorizationUrl: String
        let expiresAt: Int

        enum CodingKeys: String, CodingKey {
            case id
            case authorizationUrl = "authorization_url"
            case expiresAt = "expires_at"
        }
    }

    struct Status: Decodable { let status: String }

    public static let delays: [Double] = {
        var delay = 1.0
        return (0..<40).map { _ in
            defer { delay = min(delay * 1.5, 5) }
            return delay
        }
    }()

    let api: HitoAPI

    public init(api: HitoAPI) {
        self.api = api
    }

    public func begin(_ connection: String, revision: Int) async throws -> Attempt {
        let begun: Begun
        do {
            begun = try await api.postExplained(["api", "v1", "plugins", connection, "authorization"], body: Revision(revision: revision), as: Begun.self)
        } catch let refusal as HitoAPI.Refusal {
            throw Refused(reason: refusal.reason)
        }
        guard let url = URL(string: begun.authorizationUrl), url.scheme == "https" || url.scheme == "http" else { throw HitoAPI.Problem.refused(422) }
        return Attempt(id: begun.id, url: url, expires: Date(timeIntervalSince1970: TimeInterval(begun.expiresAt)))
    }

    public func status(_ connection: String, attempt: String) async throws -> State {
        do {
            let found = try await api.get(["api", "v1", "plugins", connection, "authorization", attempt], as: Status.self)
            switch found.status {
            case "completed": return .completed
            case "failed": return .failed
            default: return .waiting
            }
        } catch HitoAPI.Problem.refused(404) {
            return .gone
        }
    }
}
