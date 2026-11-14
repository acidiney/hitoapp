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

    let request = UUID(uuidString: "11111111-2222-4333-8444-555555555555")!
    let spotify = PluginConnection(id: "i2", name: "Spotify public playlists", capabilities: ["playlists"], health: .ok, revision: 3, checkedAt: nil)
    let youtube = PluginConnection(id: "i1", name: "YouTube", capabilities: ["playlists", "search"], health: .ok, revision: 1, checkedAt: nil)

    @Test func copyingALinkImportsItWithTheConnectionRevision() async throws {
        let fake = FakeServer(routes: ["POST /api/v1/plugins/i2/playlists/import": .json(200, #"{"request_id":"11111111-2222-4333-8444-555555555555","playlist_id":"p9"}"#)])
        let id = try await playlists(fake).add("https://open.spotify.com/playlist/abc", from: spotify, as: .copy, request: request)
        #expect(id == "p9")
        let body = try #require(fake.requests.first?.object)
        #expect(body["installation_revision"] as? Int == 3)
        #expect(body["playlist_reference"] as? String == "https://open.spotify.com/playlist/abc")
        #expect(body["request_id"] as? String == "11111111-2222-4333-8444-555555555555")
        #expect(body["interval"] == nil)
    }

    @Test func followingALinkSendsHowOftenToCheck() async throws {
        let fake = FakeServer(routes: ["POST /api/v1/plugins/i2/playlists/follow": .json(200, #"{"request_id":"11111111-2222-4333-8444-555555555555","playlist_id":"p9"}"#)])
        _ = try await playlists(fake).add("https://open.spotify.com/playlist/abc", from: spotify, as: .follow(.daily), request: request)
        #expect(fake.requests.first?.object?["interval"] as? String == "daily")
    }

    @Test func aReceiptForAnotherRequestIsNotTrusted() async {
        let fake = FakeServer(routes: ["POST /api/v1/plugins/i2/playlists/import": .json(200, #"{"request_id":"99999999-2222-4333-8444-555555555555","playlist_id":"p9"}"#)])
        await #expect(throws: PlaylistLink.Problem.self) {
            try await playlists(fake).add("https://open.spotify.com/playlist/abc", from: spotify, as: .copy, request: request)
        }
    }

    @Test func aLinkPicksTheConnectionForItsService() {
        #expect(PlaylistLink.connection(for: "https://open.spotify.com/playlist/abc", among: [youtube, spotify]) == spotify)
        #expect(PlaylistLink.connection(for: "music.youtube.com/playlist?list=PL1", among: [youtube, spotify]) == youtube)
        #expect(PlaylistLink.connection(for: "https://example.com/list", among: [spotify]) == spotify)
        #expect(PlaylistLink.connection(for: "https://example.com/list", among: [youtube, spotify]) == nil)
        #expect(PlaylistLink.service(of: "spotify:playlist:abc") == "Spotify")
    }

    @Test func importProblemsSayWhatHappened() {
        #expect(PlaylistLink.problem(HitoAPI.Problem.refused(404), service: "Spotify", following: true) == "Spotify didn't return this playlist. It may be private, a personal mix, or deleted. Nothing was saved.")
        #expect(PlaylistLink.problem(HitoAPI.Problem.refused(429), service: "Spotify", following: true) == "Hito can't follow more playlists right now. You can still copy this one.")
        #expect(PlaylistLink.problem(HitoAPI.Problem.refused(429), service: "Spotify", following: false) == "Hito can't import more playlists right now. Try again later.")
        #expect(PlaylistLink.problem(HitoAPI.Problem.refused(403), service: "Spotify", following: false) == "Spotify refused. Its connection may need your review. Nothing was saved.")
        #expect(PlaylistLink.problem(HitoAPI.Problem.unexpected(503), service: "Spotify", following: false) == "The Spotify plugin is busy. Nothing was saved. Try again in a moment.")
        #expect(PlaylistLink.problem(HitoAPI.Problem.noAnswer, service: "Spotify", following: false) == "Hito didn't get an answer. Trying again won't import it twice.")
    }

    @Test func addingTracksInsertsEachOneInOrder() async throws {
        let fake = FakeServer(routes: ["POST /api/v1/sync": .acknowledging])
        try await playlists(fake).insert(["r1", "r2"], into: "p1", device: "device-1", from: 1000)
        let body = try #require(fake.requests.first?.object)
        #expect(body["device_id"] as? String == "device-1")
        let actions = try #require((body["operations"] as? [[String: Any]])?.compactMap { $0["action"] as? [String: Any] })
        #expect(actions.map { $0["kind"] as? String } == ["entry_insert", "entry_insert"])
        #expect(actions.map { $0["recording_id"] as? String } == ["r1", "r2"])
        #expect(actions.map { $0["playlist_id"] as? String } == ["p1", "p1"])
        #expect(actions.map { $0["position"] as? Int } == [1000, 1001])
        #expect(Set(actions.compactMap { $0["entry_id"] as? String }).count == 2)
    }

    @Test func anInsertTheServerDidNotAcknowledgeFails() async {
        let fake = FakeServer(routes: ["POST /api/v1/sync": .json(200, #"{"acknowledged":[]}"#)])
        await #expect(throws: Likes.Problem.notAcknowledged) {
            try await playlists(fake).insert(["r1"], into: "p1", device: "device-1", from: 0)
        }
    }
}
