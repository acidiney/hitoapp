import Foundation

final class FakeServer: @unchecked Sendable {
    enum Answer {
        case json(Int, String)
        case text(Int, String)
        case redirect(String)
    }

    struct Request {
        let line: String
        let headers: [String: String]
        let body: Data?

        var authorization: String? { header("authorization") }

        func header(_ name: String) -> String? {
            headers.first { $0.key.lowercased() == name.lowercased() }?.value
        }

        var json: [String: String]? {
            body.flatMap { try? JSONSerialization.jsonObject(with: $0) as? [String: String] }
        }
    }

    private static let lock = NSLock()
    nonisolated(unsafe) private static var servers: [String: FakeServer] = [:]
    fileprivate static let routing = "X-Fake-Server"

    let routes: [String: Answer]
    let failures: [String: URLError]
    private var recorded: [Request] = []

    var requests: [Request] { Self.lock.withLock { recorded } }

    let session: URLSession

    init(routes: [String: Answer], failures: [String: URLError] = [:]) {
        self.routes = routes
        self.failures = failures
        let id = UUID().uuidString
        let configuration = URLSessionConfiguration.ephemeral
        configuration.protocolClasses = [FakeProtocol.self]
        configuration.httpAdditionalHeaders = [Self.routing: id]
        session = URLSession(configuration: configuration)
        Self.lock.withLock { Self.servers[id] = self }
    }

    fileprivate static func answer(_ request: URLRequest) -> (Answer?, URLError?) {
        lock.withLock {
            guard let id = request.value(forHTTPHeaderField: routing), let server = servers[id], let url = request.url else {
                return (nil, URLError(.unknown))
            }
            let method = request.httpMethod ?? "GET"
            let origin = "\(url.scheme ?? "")://\(url.host() ?? "")\(url.port.map { ":\($0)" } ?? "")"
            let full = "\(method) \(origin)\(url.path())"
            let short = "\(method) \(url.path())"
            let usesFullLines = server.routes.keys.contains { $0.contains("://") } || !server.failures.isEmpty
            server.recorded.append(Request(line: usesFullLines ? full : short, headers: request.allHTTPHeaderFields ?? [:], body: request.bodyData))
            if let failure = server.failures[origin] { return (nil, failure) }
            return (server.routes[full] ?? server.routes[short] ?? .text(404, "not found"), nil)
        }
    }
}

private extension URLRequest {
    var bodyData: Data? {
        if let httpBody { return httpBody }
        guard let stream = httpBodyStream else { return nil }
        stream.open()
        defer { stream.close() }
        var data = Data()
        var buffer = [UInt8](repeating: 0, count: 4096)
        while stream.hasBytesAvailable {
            let read = stream.read(&buffer, maxLength: buffer.count)
            if read <= 0 { break }
            data.append(buffer, count: read)
        }
        return data
    }
}

final class FakeProtocol: URLProtocol {
    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }

    override func startLoading() {
        let (answer, failure) = FakeServer.answer(request)
        if let failure {
            client?.urlProtocol(self, didFailWithError: failure)
            return
        }
        guard let answer, let url = request.url else { return }
        if case let .redirect(location) = answer {
            let target = URL(string: location)!
            let response = HTTPURLResponse(url: url, statusCode: 302, httpVersion: "HTTP/1.1", headerFields: ["Location": location])!
            var next = URLRequest(url: target)
            next.allHTTPHeaderFields = request.allHTTPHeaderFields
            client?.urlProtocol(self, wasRedirectedTo: next, redirectResponse: response)
            client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
            client?.urlProtocolDidFinishLoading(self)
            return
        }
        let (status, body, type): (Int, String, String) = switch answer {
        case let .json(status, body): (status, body, "application/json")
        case let .text(status, body): (status, body, "text/plain")
        case .redirect: (0, "", "")
        }
        let response = HTTPURLResponse(url: url, statusCode: status, httpVersion: "HTTP/1.1", headerFields: ["Content-Type": type])!
        client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: Data(body.utf8))
        client?.urlProtocolDidFinishLoading(self)
    }

    override func stopLoading() {}
}
