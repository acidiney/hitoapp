import Foundation
import Testing
@testable import HitoKit

struct GenresTests {
    let address = try! ServerAddressInput(parsing: "http://192.168.1.20:8080").candidates[0]
    let release = #"{"id":"a1","library_id":"l1","title":"Owned Album","artist":"Owned Band","artist_id":null,"track_count":2,"duration_ms":500000,"inferred_artist":null,"year":null}"#
    let recording = #"{"id":"r1","library_id":"l1","title":"Owned Song","inherited_title":"Owned Song","inherited_artist":"Owned Band","artist":"Owned Band","duration_ms":200000,"available":true,"plays_from_services":false,"liked":false,"album":"Owned Album","release_id":"a1"}"#

    func genres(_ fake: FakeServer) -> Genres {
        Genres(api: HitoAPI(address: address, token: "session-token-1", session: fake.session))
    }

    @Test func genresYouOwnListInTheWebsWords() async throws {
        let fake = FakeServer(routes: ["GET /api/v1/genres": .json(200, #"[{"key":"r&b soul","label":"r&b soul","track_count":3},{"key":"hip-hop","label":"hip-hop","track_count":1},{"key":"Kizomba","label":"Kizomba","track_count":1200}]"#)])
        let list = try await genres(fake).list()
        #expect(list.map(\.title) == ["R&B Soul", "Hip-Hop", "Kizomba"])
        #expect(list.map(\.count) == ["3 tracks", "1 track", "1,000+ tracks"])
    }

    @Test func aGenrePageAsksForItsKeyAndCarriesAlbumsAndTracks() async throws {
        let fake = FakeServer(routes: ["GET /api/v1/genres/detail": .json(200, #"{"genre":{"key":"r&b soul","label":"r&b soul","track_count":5},"albums":[\#(release)],"tracks":[\#(recording)]}"#)])
        let detail = try await genres(fake).detail("r&b soul")
        #expect(fake.requests.first?.query == "key=r%26b%20soul")
        #expect(detail.genre.title == "R&B Soul")
        #expect(detail.albums.map(\.id) == ["a1"])
        #expect(detail.tracks.map(\.id) == ["r1"])
        #expect(detail.showing == "Showing 1 of 5 tracks.")
    }

    @Test func genreTracksPageWithTheServersCursor() async throws {
        let fake = FakeServer(routes: ["GET /api/v1/genres/tracks": .sequence([
            .json(200, #"{"items":[\#(recording)],"next_after":"c1"}"#),
            .json(200, #"{"items":[],"next_after":null}"#),
        ])])
        let first = try await genres(fake).tracks("r&b soul", after: nil)
        #expect(first.items.map(\.id) == ["r1"])
        #expect(first.next == "c1")
        let last = try await genres(fake).tracks("r&b soul", after: "c1")
        #expect(last.items.isEmpty && last.next == nil)
        #expect(fake.requests.map(\.query) == ["key=r%26b%20soul&limit=100", "key=r%26b%20soul&limit=100&after=c1"])
    }
}
