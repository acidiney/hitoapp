import Foundation
import Testing
@testable import HitoKit

@Suite struct LibraryTests {
    let address = try! ServerAddressInput(parsing: "http://192.168.1.20:8080").candidates[0]

    func library(_ fake: FakeServer) -> Library {
        Library(api: HitoAPI(address: address, token: "session-token-1", session: fake.session))
    }

    let track = #"{"id":"r1","library_id":"l1","title":"Fixture 440 Hz","inherited_title":"Fixture 440 Hz","inherited_artist":"Fixture Band","artist":"Fixture Band","duration_ms":281000,"available":true,"plays_from_services":false,"liked":true,"album":"Fixture Album","release_id":"a1"}"#

    @Test func tracksArriveAPageAtATimeWithTheSessionToken() async throws {
        let fake = FakeServer(routes: ["GET /api/v1/recordings": .json(200, #"{"items":[\#(track)],"total":31,"offset":0,"limit":50}"#)])
        let page = try await library(fake).tracks(offset: 0)
        #expect(page.total == 31)
        #expect(page.items == [Track(id: "r1", title: "Fixture 440 Hz", artist: "Fixture Band", album: "Fixture Album", releaseId: "a1", durationMs: 281000, available: true, playsFromServices: false, liked: true)])
        let request = try #require(fake.requests.first)
        #expect(request.authorization == "Bearer session-token-1")
        #expect(request.query == "offset=0&limit=50")
    }

    @Test func likedTracksAndSearchesNarrowTheList() async throws {
        let fake = FakeServer(routes: ["GET /api/v1/recordings": .json(200, #"{"items":[],"total":0,"offset":0,"limit":50}"#)])
        _ = try await library(fake).tracks(offset: 50, liked: true)
        _ = try await library(fake).tracks(offset: 0, search: "fixture band")
        #expect(fake.requests.map(\.query) == ["offset=50&limit=50&liked=true", "offset=0&limit=50&q=fixture%20band"])
    }

    @Test func albumsAreListedNewestFirst() async throws {
        let fake = FakeServer(routes: ["GET /api/v1/releases": .json(200, #"{"items":[{"id":"a1","library_id":"l1","title":"Fixture Album","artist":"Fixture Band","artist_id":null,"track_count":12,"duration_ms":2400000,"inferred_artist":null,"year":2019}],"total":1,"offset":0,"limit":50}"#)])
        let page = try await library(fake).albums(offset: 0)
        #expect(page.items == [Album(id: "a1", title: "Fixture Album", artist: "Fixture Band", trackCount: 12, year: 2019)])
        #expect(fake.requests.first?.query == "offset=0&limit=50&sort=recent")
    }

    @Test func anAlbumComesWithItsTracksInOrder() async throws {
        let release = #"{"id":"a1","library_id":"l1","title":"Fixture Album","artist":null,"artist_id":null,"track_count":2,"duration_ms":500000,"inferred_artist":null,"year":null}"#
        let second = track.replacingOccurrences(of: #""id":"r1""#, with: #""id":"r2""#).dropLast() + #","entry_id":"e2","track_number":2,"disc_number":1}"#
        let first = track.dropLast() + #","entry_id":"e1","track_number":1,"disc_number":1}"#
        let fake = FakeServer(routes: ["GET /api/v1/releases/a1": .json(200, #"{"release":\#(release),"tracks":{"items":[\#(first),\#(second)],"total":2,"offset":0,"limit":200},"catalog_anchor":null,"saved_edition":null}"#)])
        let album = try await library(fake).album("a1")
        #expect(album.album == Album(id: "a1", title: "Fixture Album", artist: nil, trackCount: 2, year: nil))
        #expect(album.tracks.map(\.id) == ["r1", "r2"])
        #expect(album.tracks.map(\.trackNumber) == [1, 2])
        #expect(fake.requests.first?.query == "offset=0&limit=200")
    }

    @Test func anEndedSessionIsReportedAsSignedOut() async {
        let fake = FakeServer(routes: ["GET /api/v1/recordings": .json(401, #"{"error":"Sign in"}"#)])
        await #expect(throws: HitoAPI.Problem.signedOut) {
            try await library(fake).tracks(offset: 0)
        }
    }

    @Test func aLockedSessionIsReportedAsLocked() async {
        let fake = FakeServer(routes: ["GET /api/v1/releases": .json(423, #"{"error":"Locked"}"#)])
        await #expect(throws: HitoAPI.Problem.locked) {
            try await library(fake).albums(offset: 0)
        }
    }

    @Test func anAlbumCoverIsAskedForWithTheSessionToken() throws {
        let request = HitoAPI(address: address, token: "session-token-1").request(Album(id: "a 1", title: "", artist: nil, trackCount: 0, year: nil).artworkPath)
        #expect(request.url?.absoluteString == "http://192.168.1.20:8080/api/v1/releases/a%201/artwork")
        #expect(request.value(forHTTPHeaderField: "Authorization") == "Bearer session-token-1")
    }
}
