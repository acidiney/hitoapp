import Foundation

public struct Genre: Decodable, Equatable, Hashable, Identifiable, Sendable {
    public let key: String
    public let label: String
    public let trackCount: Int

    public var id: String { key }

    enum CodingKeys: String, CodingKey {
        case key, label
        case trackCount = "track_count"
    }

    static let acronyms: Set<String> = ["r&b", "edm", "uk", "us", "dj", "mpb"]
    static let countCap = 1000

    public var title: String {
        guard label.rangeOfCharacter(from: .uppercaseLetters) == nil else { return label }
        var words: [String] = []
        var current = ""
        for character in label {
            if character == " " || character == "-" {
                words.append(Self.titled(current))
                words.append(String(character))
                current = ""
            } else {
                current.append(character)
            }
        }
        words.append(Self.titled(current))
        return words.joined()
    }

    public var shortCount: String {
        trackCount >= Self.countCap ? "\(Self.countCap.formatted(.number.locale(Locale(identifier: "en_US"))))+" : String(trackCount)
    }

    public var count: String {
        "\(shortCount) \(trackCount == 1 ? "track" : "tracks")"
    }

    static func titled(_ word: String) -> String {
        if acronyms.contains(word) { return word.uppercased() }
        return word.prefix(1).uppercased() + word.dropFirst()
    }
}

public struct GenreDetail: Decodable, Equatable, Sendable {
    public let genre: Genre
    public let albums: [Album]
    public let tracks: [Track]

    public static let previewLimit = 100

    public var mayHaveMore: Bool { tracks.count >= Self.previewLimit }
}

public struct Genres: Sendable {
    let api: HitoAPI

    public init(api: HitoAPI) {
        self.api = api
    }

    public func list() async throws -> [Genre] {
        try await api.get(["api", "v1", "genres"], as: [Genre].self)
    }

    public struct TracksPage: Equatable, Sendable {
        public let items: [Track]
        public let next: String?
    }

    struct WireTracks: Decodable {
        let items: [Track]
        let nextAfter: String?

        enum CodingKeys: String, CodingKey {
            case items
            case nextAfter = "next_after"
        }
    }

    public func tracks(_ key: String, after: String?) async throws -> TracksPage {
        var query = [URLQueryItem(name: "key", value: key), URLQueryItem(name: "limit", value: "100")]
        if let after { query.append(URLQueryItem(name: "after", value: after)) }
        let page = try await api.get(["api", "v1", "genres", "tracks"], query: query, as: WireTracks.self)
        return TracksPage(items: page.items, next: page.nextAfter)
    }

    public func detail(_ key: String) async throws -> GenreDetail {
        try await api.get(["api", "v1", "genres", "detail"], query: [URLQueryItem(name: "key", value: key)], as: GenreDetail.self)
    }
}
