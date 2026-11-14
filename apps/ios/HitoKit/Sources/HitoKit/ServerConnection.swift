import Foundation

public enum NextStep: Equatable, Sendable {
    case createAdministrator
    case signIn
}

public struct FoundServer: Equatable, Sendable {
    public let address: ServerAddress
    public let next: NextStep

    public init(address: ServerAddress, next: NextStep) {
        self.address = address
        self.next = next
    }
}

public struct ServerConnection: Sendable {
    public enum Problem: Error, Equatable {
        case noAnswer
        case certificate
        case notHito
        case notReady
    }

    struct Health: Decodable { let ready: Bool }
    struct SetupStatus: Decodable {
        let needsSetup: Bool
        enum CodingKeys: String, CodingKey { case needsSetup = "needs_setup" }
    }

    let session: URLSession

    public init(session: URLSession = .shared) {
        self.session = session
    }

    public func find(_ input: ServerAddressInput) async throws -> FoundServer {
        var last = Problem.noAnswer
        for address in input.candidates {
            try Task.checkCancellation()
            let health: Health
            do {
                health = try await read(address.url("health"), from: address)
            } catch Problem.noAnswer {
                last = .noAnswer
                continue
            }
            return FoundServer(address: address, next: try await nextStep(at: address, after: health))
        }
        throw last
    }

    public func check(_ address: ServerAddress) async throws -> NextStep {
        try await nextStep(at: address, after: try await read(address.url("health"), from: address))
    }

    private func nextStep(at address: ServerAddress, after health: Health) async throws -> NextStep {
        guard health.ready else { throw Problem.notReady }
        let setup: SetupStatus = try await read(address.url("api/v1/setup"), from: address)
        return setup.needsSetup ? .createAdministrator : .signIn
    }

    private func read<Value: Decodable>(_ url: URL, from address: ServerAddress) async throws -> Value {
        var request = URLRequest(url: url, timeoutInterval: 10)
        request.setValue("application/json", forHTTPHeaderField: "Accept")
        let (data, response) = try await Self.load(request, with: session, staying: address)
        guard response.statusCode == 200, let value = try? JSONDecoder().decode(Value.self, from: data) else {
            throw Problem.notHito
        }
        return value
    }

    static let agent = "HitoiOS/\(Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? "0")"

    static func load(_ request: URLRequest, with session: URLSession, staying address: ServerAddress) async throws -> (Data, HTTPURLResponse) {
        var request = request
        request.setValue(agent, forHTTPHeaderField: "User-Agent")
        do {
            let (data, response) = try await session.data(for: request, delegate: SameServer(address: address))
            guard let http = response as? HTTPURLResponse else { throw Problem.notHito }
            return (data, http)
        } catch let error as URLError {
            if error.code == .cancelled { throw CancellationError() }
            throw problem(for: error)
        }
    }

    static func problem(for error: URLError) -> Problem {
        switch error.code {
        case .serverCertificateUntrusted, .serverCertificateHasBadDate, .serverCertificateNotYetValid,
             .serverCertificateHasUnknownRoot, .clientCertificateRejected, .clientCertificateRequired:
            return .certificate
        case .timedOut, .cannotFindHost, .cannotConnectToHost, .networkConnectionLost, .dnsLookupFailed,
             .notConnectedToInternet, .secureConnectionFailed, .cannotLoadFromNetwork, .internationalRoamingOff,
             .dataNotAllowed, .callIsActive, .appTransportSecurityRequiresSecureConnection:
            return .noAnswer
        default:
            return .notHito
        }
    }
}

final class SameServer: NSObject, URLSessionTaskDelegate, Sendable {
    let address: ServerAddress

    init(address: ServerAddress) {
        self.address = address
    }

    func urlSession(
        _ session: URLSession,
        task: URLSessionTask,
        willPerformHTTPRedirection response: HTTPURLResponse,
        newRequest request: URLRequest
    ) async -> URLRequest? {
        address.isSameServer(as: request.url) ? request : nil
    }
}
