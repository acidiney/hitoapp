import Foundation
import Testing
@testable import HitoKit

@Suite struct PlaylistsTests {
    let address = try! ServerAddressInput(parsing: "http://192.168.1.20:8080").candidates[0]
    let entry = #"{"entry_id":"e1","position":0,"recording_id":"r1","title":"Fixture 440 Hz","inherited_title":"Fixture 440 Hz","artist":"Fixture Band","inherited_artist":"Fixture Band","album":null,"release_id":null,"duration_ms":90000,"available":true,"plays_from_services":false,"liked":false}"#
    let missing = #"{"entry_id":"e2","position":1,"recording_id":null,"title":"Gone Song","inherited_title":"Gone Song","artist":"Someone","inherited_artist":"Someone","album":null,"release_id":null,"duration_ms":120000,"available":false,"plays_from_services":false,"liked":false}"#

    func playlists(_ fake: FakeServer) -> Playlists {
        Playlists(api: HitoAPI(address: address, token: "session-token-1", session: fake.session))
    }

    @Test func playlistsAreListed() async throws {
        let fake = FakeServer(routes: ["GET /api/v1/playlists": .json(200, #"[{"id":"p1","name":"Road trip","kind":"local"},{"id":"p2","name":"Banger","kind":"followed"}]"#)])
        let listed = try await playlists(fake).list()
        #expect(listed == [Playlist(id: "p1", name: "Road trip", kind: .local), Playlist(id: "p2", name: "Banger", kind: .followed)])
    }

    @Test func aPlaylistOpensWithItsEntriesAndCounts() async throws {
        let body = #"{"playlist":{"id":"p2","name":"Banger","kind":"followed"},"entries":[\#(entry),\#(missing)],"origin":{"installation_id":"i1","installation_revision":1,"service_id":"s1","service_revision":1,"provider":"spotify","provider_playlist_id":"x","snapshot":"y","attribution":{"label":"Spotify"},"imported_at":1}}"#
        let fake = FakeServer(routes: ["GET /api/v1/playlists/p2": .json(200, body)])
        let detail = try await playlists(fake).detail("p2")
        #expect(detail.playlist.name == "Banger")
        #expect(detail.origin == "Followed from Spotify")
        #expect(detail.tracks.map(\.title) == ["Fixture 440 Hz", "Gone Song"])
        #expect(detail.tracks.map(\.isPlayable) == [true, false])
        #expect(detail.ready == 1)
    }

    @Test func aCopiedPlaylistSaysWhereItCameFrom() async throws {
        let body = #"{"playlist":{"id":"p1","name":"Copy","kind":"local"},"entries":[],"origin":{"installation_id":"i1","installation_revision":1,"service_id":"s1","service_revision":1,"provider":"spotify","provider_playlist_id":"x","snapshot":"y","attribution":{"label":"Spotify"},"imported_at":1}}"#
        let fake = FakeServer(routes: ["GET /api/v1/playlists/p1": .json(200, body)])
        #expect(try await playlists(fake).detail("p1").origin == "A copy of Spotify, yours to edit")
    }

    @Test func aPlaylistYouMadeSaysSo() async throws {
        let fake = FakeServer(routes: ["GET /api/v1/playlists/p1": .json(200, #"{"playlist":{"id":"p1","name":"Mine","kind":"local"},"entries":[]}"#)])
        #expect(try await playlists(fake).detail("p1").origin == "Your playlist")
    }
}
