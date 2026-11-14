import Foundation
import Testing
@testable import HitoKit

@Suite struct HomeSummaryTests {
    let address = try! ServerAddressInput(parsing: "http://192.168.1.20:8080").candidates[0]

    func home(_ fake: FakeServer) -> HomeFeed {
        HomeFeed(api: HitoAPI(address: address, token: "session-token-1", session: fake.session))
    }

    func album(_ id: String) -> String {
        #"{"id":"\#(id)","title":"Album \#(id)","artist":"Band","inferred_artist":null,"last_played_at":10,"played":4}"#
    }

    var body: String {
        let recording = #"{"id":"r9","library_id":"l1","title":"Unheard Song","inherited_title":"Unheard Song","inherited_artist":"Band","artist":"Band","duration_ms":200000,"available":true,"plays_from_services":false,"liked":false,"album":null,"release_id":null}"#
        return #"{"has_music":true,"liked_count":12,"recent_albums":[\#(["a1","a2","a3","a4","a5"].map(album).joined(separator: ","))],"recently_added":[],"unheard":[{"release_id":"a1","title":"Album a1","artist":"Band","inferred_artist":null,"played":11,"remaining":3,"tracks":[\#(recording)]}]}"#
    }

    @Test func homeFillsSixTilesLikeTheWeb() async throws {
        let fake = FakeServer(routes: ["GET /api/v1/home": .json(200, body)])
        let summary = try #require(try await home(fake).load())
        let playlists = [Playlist(id: "p1", name: "Mix", kind: .local)]
        #expect(summary.tiles(playlists: playlists).map(\.title) == ["Album a1", "Album a2", "Album a3", "Liked tracks", "Mix", "Album a4"])
    }

    @Test func theUnheardCardSaysWhy() async throws {
        let fake = FakeServer(routes: ["GET /api/v1/home": .json(200, body)])
        let card = try #require(try await home(fake).load()?.unheard.first)
        #expect(card.headline == "3 more tracks from Album a1")
        #expect(card.reason == "You played 11 tracks from this album. These are the ones you haven't heard yet.")
        #expect(card.tracks.map(\.id) == ["r9"])
    }

    @Test func anOlderServerWithoutHomeFallsBack() async throws {
        let fake = FakeServer(routes: ["GET /api/v1/home": .json(404, #"{"error":"Resource not found"}"#)])
        #expect(try await home(fake).load() == nil)
    }
}
