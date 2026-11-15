import Foundation

public enum SearchScope: String, CaseIterable, Identifiable, Sendable {
    case all, tracks, albums, artists

    public enum Top: Equatable, Sendable {
        case artist, track, album
    }

    public static let trackPreview = 8

    public var id: String { rawValue }

    public var label: String {
        switch self {
        case .all: "All"
        case .tracks: "Tracks"
        case .albums: "Albums"
        case .artists: "Artists"
        }
    }

    static func fold(_ text: String) -> String {
        text.folding(options: [.diacriticInsensitive, .caseInsensitive], locale: .current).trimmingCharacters(in: .whitespacesAndNewlines)
    }

    public static func topKind(query: String, tracks: Int, albums: Int, artistNames: [String]) -> Top? {
        let wanted = fold(query)
        if artistNames.contains(where: { fold($0) == wanted }) { return .artist }
        if tracks > 0 { return .track }
        if albums > 0 { return .album }
        return artistNames.isEmpty ? nil : .artist
    }
}
