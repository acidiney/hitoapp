import CryptoKit
import Foundation

public struct PluginSignIn: Sendable {
    public static let callbackScheme = "hito"

    public struct Verifier: Equatable, Sendable, Codable {
        public let value: String

        public init() {
            var generator = SystemRandomNumberGenerator()
            value = Self.base64URL(Data((0..<32).map { _ in UInt8.random(in: .min ... .max, using: &generator) }))
        }

        init(value: String) {
            self.value = value
        }

        public var hash: String { Self.base64URL(Data(SHA256.hash(data: Data(value.utf8)))) }

        static func base64URL(_ data: Data) -> String {
            data.base64EncodedString()
                .replacingOccurrences(of: "+", with: "-")
                .replacingOccurrences(of: "/", with: "_")
                .replacingOccurrences(of: "=", with: "")
        }
    }

    public struct Attempt: Equatable, Sendable, Codable {
        public let id: String
        public let connection: String
        public let expires: Date
        public let verifier: Verifier
        public var returnSecret: String?
    }

    public struct Refused: Error, Equatable {
        public let reason: String?
    }

    public struct OnlyOnTheWeb: Error, Equatable {}

    public enum Outcome: String, Equatable, Sendable {
        case connected, denied, expired, changed, used, invalid
        case providerError = "provider_error"
        case exchangeFailed = "exchange_failed"

        public func problem(_ name: String) -> String? {
            switch self {
            case .connected: nil
            case .denied: "\(name) did not grant access. Nothing was connected."
            case .providerError: "\(name) could not complete the sign-in. Try again."
            case .exchangeFailed: "\(name) did not accept the approval. Try again."
            case .expired: "This sign-in expired. Try again."
            case .changed: "\(name) or your Hito sign-in changed while you were approving. Try again."
            case .used: "This sign-in was already used. Try again if you still need to connect."
            case .invalid: "This sign-in could not be confirmed. Try again."
            }
        }
    }

    public enum Resumption: Equatable, Sendable {
        case finished(Outcome)
        case waiting
        case notReturned
        case ended
    }

    struct Start: Encodable {
        let revision: Int
        let verifierHash: String

        enum CodingKeys: String, CodingKey {
            case revision
            case verifierHash = "verifier_hash"
        }
    }

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

    struct Completion: Encodable {
        let verifier: String
        let returnSecret: String

        enum CodingKeys: String, CodingKey {
            case verifier
            case returnSecret = "return_secret"
        }
    }

    struct Result: Decodable {
        let outcome: String
    }

    struct Status: Decodable { let status: String }

    static let olderServer = "Request JSON does not match the expected fields"

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

    public static func webSettings(on address: ServerAddress) -> URL {
        var components = URLComponents(url: address.url(["settings"]), resolvingAgainstBaseURL: false)!
        components.queryItems = [URLQueryItem(name: "section", value: "plugins")]
        return components.url!
    }

    public static func returnSecret(from url: URL, for attempt: String) -> String? {
        guard let components = URLComponents(url: url, resolvingAgainstBaseURL: false),
              components.scheme == callbackScheme,
              components.host == "plugin-authorization",
              components.user == nil, components.password == nil, components.port == nil,
              components.path == "/return",
              components.fragment == nil
        else { return nil }
        let items = components.queryItems ?? []
        let attempts = items.filter { $0.name == "attempt" }
        let secrets = items.filter { $0.name == "return" }
        guard attempts.count == 1, attempts[0].value == attempt,
              secrets.count == 1, let secret = secrets[0].value, !secret.isEmpty
        else { return nil }
        return secret
    }

    public func begin(_ connection: String, revision: Int) async throws -> (attempt: Attempt, url: URL) {
        let verifier = Verifier()
        let begun: Begun
        do {
            begun = try await api.postExplained(["api", "v1", "plugins", connection, "authorization"], body: Start(revision: revision, verifierHash: verifier.hash), as: Begun.self)
        } catch let refusal as HitoAPI.Refusal {
            if refusal.status == 422, refusal.reason == Self.olderServer { throw OnlyOnTheWeb() }
            throw Refused(reason: refusal.reason)
        }
        guard let url = URL(string: begun.authorizationUrl), url.scheme == "https" || url.scheme == "http" else { throw HitoAPI.Problem.refused(422) }
        return (Attempt(id: begun.id, connection: connection, expires: Date(timeIntervalSince1970: TimeInterval(begun.expiresAt)), verifier: verifier), url)
    }

    public func complete(_ attempt: Attempt, returnSecret: String) async throws -> Outcome {
        let result = try await api.post(
            ["api", "v1", "plugins", attempt.connection, "authorization", attempt.id, "completion"],
            body: Completion(verifier: attempt.verifier.value, returnSecret: returnSecret),
            as: Result.self
        )
        return Outcome(rawValue: result.outcome) ?? .invalid
    }

    public func resume(_ attempt: Attempt) async throws -> Resumption {
        let found: Status
        do {
            found = try await api.get(["api", "v1", "plugins", attempt.connection, "authorization", attempt.id], as: Status.self)
        } catch HitoAPI.Problem.refused(404) {
            return .ended
        }
        switch found.status {
        case "pending": return .notReturned
        case "returned":
            guard let secret = attempt.returnSecret else { return .ended }
            return .finished(try await complete(attempt, returnSecret: secret))
        case "completed": return .finished(.connected)
        case "failed": return .ended
        default: return .waiting
        }
    }
}
