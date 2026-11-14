import Foundation

public struct ServerAddress: Hashable, Sendable, Codable {
    public let origin: URL

    public var isEncrypted: Bool { origin.scheme == "https" }

    public var host: String { origin.host() ?? "" }

    public func url(_ path: String) -> URL {
        origin.appending(path: path)
    }

    func isSameServer(as url: URL?) -> Bool {
        guard let url else { return false }
        return url.scheme == origin.scheme && url.host() == origin.host() && url.port == origin.port
    }
}

public struct ServerAddressInput: Sendable {
    public enum Problem: Error, Equatable {
        case notAnAddress
    }

    public let candidates: [ServerAddress]
    public let display: String

    public init(parsing input: String) throws {
        let trimmed = input.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty, !trimmed.contains(where: \.isWhitespace) else { throw Problem.notAnAddress }
        let typedScheme = trimmed.range(of: "://") != nil
        guard let parsed = URLComponents(string: typedScheme ? trimmed : "https://\(trimmed)"),
              let scheme = parsed.scheme?.lowercased(), scheme == "http" || scheme == "https",
              parsed.user == nil, parsed.password == nil,
              let host = parsed.host?.lowercased(), Self.isValidHost(host)
        else { throw Problem.notAnAddress }
        let schemes = typedScheme ? [scheme] : ["https", "http"]
        candidates = try schemes.map { scheme in
            var origin = URLComponents()
            origin.scheme = scheme
            origin.host = host
            origin.port = parsed.port
            guard let url = origin.url else { throw Problem.notAnAddress }
            return ServerAddress(origin: url)
        }
        display = parsed.port.map { "\(host):\($0)" } ?? host
    }

    static func isValidHost(_ host: String) -> Bool {
        if host.hasPrefix("[") || host.contains(":") {
            let inner = host.trimmingCharacters(in: CharacterSet(charactersIn: "[]"))
            return inner.contains(":") && inner.allSatisfy { $0.isHexDigit || $0 == ":" || $0 == "." }
        }
        let labels = host.split(separator: ".", omittingEmptySubsequences: false)
        return labels.allSatisfy { label in
            !label.isEmpty && label.count <= 63 && !label.hasPrefix("-") && !label.hasSuffix("-")
                && label.allSatisfy { $0.isASCII && ($0.isLetter || $0.isNumber || $0 == "-") }
        }
    }
}
