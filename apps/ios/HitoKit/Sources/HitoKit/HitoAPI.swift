import Foundation

public struct HitoAPI: Sendable {
    public enum Problem: Error, Equatable {
        case signedOut
        case locked
        case noAnswer
        case certificate
        case refused(Int)
        case unexpected(Int)
    }

    public let address: ServerAddress
    let token: String
    let session: URLSession

    public init(address: ServerAddress, token: String, session: URLSession = .shared) {
        self.address = address
        self.token = token
        self.session = session
    }

    public var authorization: [String: String] { ["Authorization": "Bearer \(token)"] }

    public func request(_ path: [String], query: [URLQueryItem] = []) -> URLRequest {
        var components = URLComponents(url: address.url(path), resolvingAgainstBaseURL: false)!
        if !query.isEmpty { components.queryItems = query }
        var request = URLRequest(url: components.url!, timeoutInterval: 20)
        request.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
        return request
    }

    func get<Value: Decodable>(_ path: [String], query: [URLQueryItem] = [], as type: Value.Type) async throws -> Value {
        var request = request(path, query: query)
        request.setValue("application/json", forHTTPHeaderField: "Accept")
        return try await send(request, as: type)
    }

    func post<Body: Encodable, Value: Decodable>(_ path: [String], body: Body, as type: Value.Type) async throws -> Value {
        try await change("POST", path, body: body, as: type)
    }

    func put<Body: Encodable, Value: Decodable>(_ path: [String], body: Body, as type: Value.Type) async throws -> Value {
        try await change("PUT", path, body: body, as: type)
    }

    func delete<Value: Decodable>(_ path: [String], as type: Value.Type) async throws -> Value {
        var request = request(path)
        request.httpMethod = "DELETE"
        request.setValue("1", forHTTPHeaderField: "x-hito-request")
        request.setValue("application/json", forHTTPHeaderField: "Accept")
        return try await send(request, as: type)
    }

    func delete<Body: Encodable, Value: Decodable>(_ path: [String], body: Body, as type: Value.Type) async throws -> Value {
        try await change("DELETE", path, body: body, as: type)
    }

    private func change<Body: Encodable, Value: Decodable>(_ method: String, _ path: [String], body: Body, as type: Value.Type) async throws -> Value {
        var request = request(path)
        request.httpMethod = method
        request.setValue("1", forHTTPHeaderField: "x-hito-request")
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.setValue("application/json", forHTTPHeaderField: "Accept")
        request.httpBody = try JSONEncoder().encode(body)
        return try await send(request, as: type)
    }

    private func send<Value: Decodable>(_ request: URLRequest, as type: Value.Type) async throws -> Value {
        let (data, response): (Data, HTTPURLResponse)
        do {
            (data, response) = try await ServerConnection.load(request, with: session, staying: address)
        } catch ServerConnection.Problem.certificate {
            throw Problem.certificate
        } catch is ServerConnection.Problem {
            throw Problem.noAnswer
        }
        switch response.statusCode {
        case 200..<300:
            if let nothing = Nothing() as? Value { return nothing }
            guard let value = try? JSONDecoder().decode(type, from: data) else {
                throw Problem.unexpected(response.statusCode)
            }
            return value
        case 401: throw Problem.signedOut
        case 423: throw Problem.locked
        case 400..<500: throw Problem.refused(response.statusCode)
        default: throw Problem.unexpected(response.statusCode)
        }
    }
}

struct Nothing: Decodable {}

public struct Page<Item: Decodable & Equatable & Sendable>: Decodable, Equatable, Sendable {
    public let items: [Item]
    public let total: Int
    public let offset: Int
}
