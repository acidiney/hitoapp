import Foundation

public struct User: Codable, Equatable, Sendable {
    public let id: String
    public let username: String
    public let admin: Bool
    public let displayName: String?

    public init(id: String, username: String, admin: Bool, displayName: String?) {
        self.id = id
        self.username = username
        self.admin = admin
        self.displayName = displayName
    }

    enum CodingKeys: String, CodingKey {
        case id, username, admin
        case displayName = "display_name"
    }
}

public struct SignedIn: Codable, Equatable, Sendable {
    public let token: String
    public let user: User
}

public struct LockStatus: Codable, Equatable, Sendable {
    public let pinEnabled: Bool
    public let idleMinutes: Int
    public let locked: Bool

    enum CodingKeys: String, CodingKey {
        case pinEnabled = "pin_enabled"
        case idleMinutes = "idle_minutes"
        case locked
    }
}

public enum UnlockResult: Equatable, Sendable {
    case unlocked
    case wrongPin
    case signedOut
}

public struct AccountSession: Sendable {
    public enum Problem: Error, Equatable {
        case locked
        case alreadySetUp
        case wrongCredentials
        case tooManyAttempts(String)
        case refused(String)
        case signedOut
        case noAnswer
        case certificate
        case unexpected(Int)
    }

    struct ServerError: Decodable { let error: String }

    let session: URLSession

    public init(session: URLSession = .shared) {
        self.session = session
    }

    public func createAdministrator(at address: ServerAddress, username: String, password: String) async throws -> SignedIn {
        try await submit("api/v1/setup", at: address, username: username, password: password, success: 201)
    }

    public func signIn(at address: ServerAddress, username: String, password: String) async throws -> SignedIn {
        try await submit("api/v1/login", at: address, username: username, password: password, success: 200)
    }

    public func currentUser(at address: ServerAddress, token: String) async throws -> User {
        var request = URLRequest(url: address.url("api/v1/me"), timeoutInterval: 15)
        request.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
        request.setValue("application/json", forHTTPHeaderField: "Accept")
        let (data, response) = try await load(request, at: address)
        switch response.statusCode {
        case 200: return try decode(User.self, data, response)
        case 401, 403: throw Problem.signedOut
        case 423: throw Problem.locked
        default: throw Problem.unexpected(response.statusCode)
        }
    }

    public func lockStatus(at address: ServerAddress, token: String) async throws -> LockStatus {
        var request = URLRequest(url: address.url("api/v1/account/lock"), timeoutInterval: 15)
        request.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
        request.setValue("application/json", forHTTPHeaderField: "Accept")
        let (data, response) = try await load(request, at: address)
        switch response.statusCode {
        case 200: return try decode(LockStatus.self, data, response)
        case 401, 403: throw Problem.signedOut
        default: throw Problem.unexpected(response.statusCode)
        }
    }

    public func unlock(at address: ServerAddress, token: String, pin: String) async throws -> UnlockResult {
        var request = URLRequest(url: address.url("api/v1/account/unlock"), timeoutInterval: 15)
        request.httpMethod = "POST"
        request.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
        request.setValue("1", forHTTPHeaderField: "x-hito-request")
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.setValue("application/json", forHTTPHeaderField: "Accept")
        request.httpBody = try JSONEncoder().encode(["pin": pin])
        let (data, response) = try await load(request, at: address)
        switch response.statusCode {
        case 200: return try decode(LockStatus.self, data, response).locked ? .wrongPin : .unlocked
        case 401, 403: return .signedOut
        case 400, 422: return .wrongPin
        default: throw Problem.unexpected(response.statusCode)
        }
    }

    public func signOut(at address: ServerAddress, token: String) async throws {
        var request = URLRequest(url: address.url("api/v1/logout"), timeoutInterval: 10)
        request.httpMethod = "POST"
        request.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
        request.setValue("1", forHTTPHeaderField: "x-hito-request")
        let (_, response) = try await load(request, at: address)
        guard (200..<300).contains(response.statusCode) || response.statusCode == 401 else {
            throw Problem.unexpected(response.statusCode)
        }
    }

    private func submit(_ path: String, at address: ServerAddress, username: String, password: String, success: Int) async throws -> SignedIn {
        var request = URLRequest(url: address.url(path), timeoutInterval: 15)
        request.httpMethod = "POST"
        request.setValue("1", forHTTPHeaderField: "x-hito-request")
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.setValue("application/json", forHTTPHeaderField: "Accept")
        request.httpBody = try JSONEncoder().encode(["username": username, "password": password])
        let (data, response) = try await load(request, at: address)
        let message = (try? JSONDecoder().decode(ServerError.self, from: data))?.error
        switch response.statusCode {
        case success: return try decode(SignedIn.self, data, response)
        case 401: throw Problem.wrongCredentials
        case 409: throw Problem.alreadySetUp
        case 429: throw Problem.tooManyAttempts(message ?? "Too many attempts. Try again in a minute.")
        case 400, 422: throw Problem.refused(message ?? "The server refused this.")
        default: throw Problem.unexpected(response.statusCode)
        }
    }

    private func load(_ request: URLRequest, at address: ServerAddress) async throws -> (Data, HTTPURLResponse) {
        do {
            return try await ServerConnection.load(request, with: session, staying: address)
        } catch ServerConnection.Problem.certificate {
            throw Problem.certificate
        } catch is ServerConnection.Problem {
            throw Problem.noAnswer
        }
    }

    private func decode<Value: Decodable>(_ type: Value.Type, _ data: Data, _ response: HTTPURLResponse) throws -> Value {
        guard let value = try? JSONDecoder().decode(type, from: data) else {
            throw Problem.unexpected(response.statusCode)
        }
        return value
    }
}
