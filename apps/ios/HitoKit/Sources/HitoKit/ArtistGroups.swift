import Foundation

public struct ArtistGroup: Decodable, Equatable, Hashable, Identifiable, Sendable {
    public let id: String
    public let name: String
    public let recordingId: String
    public let associationId: String
    public let recordingCount: Int
    public let releaseCount: Int

    public init(id: String, name: String, recordingId: String, associationId: String, recordingCount: Int, releaseCount: Int) {
        self.id = id
        self.name = name
        self.recordingId = recordingId
        self.associationId = associationId
        self.recordingCount = recordingCount
        self.releaseCount = releaseCount
    }

    public init(from decoder: Decoder) throws {
        let read = try ArtistGroups.Group(from: decoder)
        self.init(id: read.id, name: read.name.value, recordingId: read.name.recordingId, associationId: read.name.associationId, recordingCount: read.recordingCount, releaseCount: read.releaseCount)
    }

    public var artworkPath: [String] { ["api", "v1", "recordings", recordingId, "artwork"] }
}

public struct ArtistGroupDetail: Equatable, Sendable {
    public let group: ArtistGroup
    public let tracks: [Track]
    public let tracksTotal: Int
    public let albums: [Album]

    public var stats: String {
        ArtistPage.counts(tracks: group.recordingCount, albums: group.releaseCount, featured: 0)
    }
}

public struct ReleaseGroup: Equatable, Hashable, Identifiable, Sendable {
    public enum Kind: String, Sendable, CaseIterable {
        case album, ep, single

        var section: String {
            switch self {
            case .album: "Albums"
            case .ep: "EPs"
            case .single: "Singles"
            }
        }
    }

    public struct Section: Equatable, Sendable {
        public let title: String
        public let items: [ReleaseGroup]
    }

    public let id: String
    public let title: String
    public let kind: Kind?
    public let year: String?

    public init(id: String, title: String, kind: Kind?, year: String?) {
        self.id = id
        self.title = title
        self.kind = kind
        self.year = year
    }

    public static func sections(_ items: [ReleaseGroup]) -> [Section] {
        let kinds = Kind.allCases.map { kind in Section(title: kind.section, items: items.filter { $0.kind == kind }) }
        let other = Section(title: "Other", items: items.filter { $0.kind == nil })
        return (kinds + [other]).filter { !$0.items.isEmpty }
    }
}

public enum DiscographyRead: Equatable, Sendable {
    case available([ReleaseGroup], continuation: String?)
    case unavailable(String)
}

public struct ArtistGroups: Sendable {
    static let pageSize = 50

    struct Name: Decodable {
        let value: String
        let recordingId: String
        let associationId: String

        enum CodingKeys: String, CodingKey {
            case value
            case recordingId = "recording_id"
            case associationId = "association_id"
        }
    }

    struct Group: Decodable {
        let id: String
        let name: Name
        let recordingCount: Int
        let releaseCount: Int

        enum CodingKeys: String, CodingKey {
            case id, name
            case recordingCount = "recording_count"
            case releaseCount = "release_count"
        }
    }

    struct Member: Decodable, Equatable, Sendable {
        let recording: Track
    }

    struct Detail: Decodable {
        let group: ArtistGroup
        let recordings: Page<Member>
        let releases: Page<Album>
    }

    struct Read: Encodable {
        let limit: Int
        let continuation: String?

        func encode(to encoder: Encoder) throws {
            var container = encoder.container(keyedBy: CodingKeys.self)
            try container.encode(limit, forKey: .limit)
            try container.encodeIfPresent(continuation, forKey: .continuation)
        }

        enum CodingKeys: String, CodingKey {
            case limit, continuation
        }
    }

    struct Listed: Decodable {
        struct Item: Decodable {
            let id: String
            let title: String
            let kind: String?
            let firstReleaseDate: String?

            enum CodingKeys: String, CodingKey {
                case id, title, kind
                case firstReleaseDate = "first_release_date"
            }
        }

        struct Content: Decodable {
            let items: [Item]
        }

        let status: String
        let continuation: String?
        let page: Content?
    }

    struct Owned: Encodable {
        let releaseGroupIds: [String]

        enum CodingKeys: String, CodingKey {
            case releaseGroupIds = "release_group_ids"
        }
    }

    struct Holdings: Decodable {
        struct Entry: Decodable {
            struct Album: Decodable {
                let releaseId: String

                enum CodingKeys: String, CodingKey {
                    case releaseId = "release_id"
                }
            }

            let releaseGroupId: String
            let albums: [Album]

            enum CodingKeys: String, CodingKey {
                case releaseGroupId = "release_group_id"
                case albums
            }
        }

        let groups: [Entry]
    }

    let api: HitoAPI

    public init(api: HitoAPI) {
        self.api = api
    }

    public func detail(_ id: String) async throws -> ArtistGroupDetail {
        let read = try await api.get(["api", "v1", "artist-identity-groups", id], as: Detail.self)
        return ArtistGroupDetail(group: read.group, tracks: read.recordings.items.map(\.recording), tracksTotal: read.recordings.total, albums: read.releases.items)
    }

    public func named(_ credit: String) async throws -> [ArtistGroup] {
        let query = [URLQueryItem(name: "q", value: credit), URLQueryItem(name: "limit", value: "20")]
        let page = try await api.get(["api", "v1", "artist-identity-groups"], query: query, as: Page<ArtistGroup>.self)
        let wanted = credit.lowercased()
        return page.items.filter { $0.name.lowercased() == wanted }
    }

    public func chosen(offset: Int) async throws -> Page<ArtistGroup> {
        let query = [URLQueryItem(name: "offset", value: String(offset)), URLQueryItem(name: "limit", value: String(Library.pageSize))]
        return try await api.get(["api", "v1", "artist-identity-groups"], query: query, as: Page<ArtistGroup>.self)
    }

    public func discography(_ group: ArtistGroup, continuation: String?) async throws -> DiscographyRead {
        let listed = try await api.post(association(group) + ["discography"], body: Read(limit: Self.pageSize, continuation: continuation), as: Listed.self)
        guard listed.status == "available" else { return .unavailable(Self.explain(listed.status)) }
        let items = (listed.page?.items ?? []).map {
            ReleaseGroup(id: $0.id, title: $0.title, kind: $0.kind.flatMap(ReleaseGroup.Kind.init(rawValue:)), year: $0.firstReleaseDate.map { String($0.prefix(4)) })
        }
        return .available(items, continuation: listed.continuation)
    }

    public func library(_ group: ArtistGroup, releaseGroups: [String]) async throws -> [String: String] {
        guard !releaseGroups.isEmpty else { return [:] }
        let owned = try await api.post(association(group) + ["discography", "library"], body: Owned(releaseGroupIds: releaseGroups), as: Holdings.self)
        var albums: [String: String] = [:]
        for entry in owned.groups {
            if let first = entry.albums.first { albums[entry.releaseGroupId] = first.releaseId }
        }
        return albums
    }

    private func association(_ group: ArtistGroup) -> [String] {
        ["api", "v1", "recordings", group.recordingId, "artist-associations", group.associationId]
    }

    static func explain(_ status: String) -> String {
        switch status {
        case "disabled": "Track details from plugins are off in your settings."
        case "unsupported", "type_filter_unsupported": "This artist's catalog can't list releases."
        case "cancelled": "Loading the discography was cancelled. Try again."
        default: "Couldn't load the discography. Your own albums are still listed."
        }
    }
}
