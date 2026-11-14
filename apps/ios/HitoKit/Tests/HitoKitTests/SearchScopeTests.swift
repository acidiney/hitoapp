import Testing
@testable import HitoKit

@Suite struct SearchScopeTests {
    @Test func anExactArtistNameWinsEvenOverTracks() {
        #expect(SearchScope.topKind(query: "  Beyoncé ", tracks: 3, albums: 1, artistNames: ["Other", "beyonce"]) == .artist)
    }

    @Test func otherwiseTracksThenAlbumsThenAnyArtistLead() {
        #expect(SearchScope.topKind(query: "fix", tracks: 2, albums: 1, artistNames: ["Fixture Band"]) == .track)
        #expect(SearchScope.topKind(query: "fix", tracks: 0, albums: 1, artistNames: ["Fixture Band"]) == .album)
        #expect(SearchScope.topKind(query: "fix", tracks: 0, albums: 0, artistNames: ["Fixture Band"]) == .artist)
        #expect(SearchScope.topKind(query: "fix", tracks: 0, albums: 0, artistNames: []) == nil)
    }

    @Test func scopesReadLikeTheWebFilters() {
        #expect(SearchScope.allCases.map(\.label) == ["All", "Tracks", "Albums", "Artists"])
        #expect(SearchScope.trackPreview == 8)
    }
}
