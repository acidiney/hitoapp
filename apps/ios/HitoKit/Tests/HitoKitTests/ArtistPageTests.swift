import Foundation
import Testing
@testable import HitoKit

@Suite struct ArtistPageTests {
    let address = try! ServerAddressInput(parsing: "http://192.168.1.20:8080").candidates[0]

    func recording(_ id: String) -> String {
        #"{"id":"\#(id)","library_id":"l1","title":"Song \#(id)","inherited_title":"Song","inherited_artist":"Band","artist":"Band","duration_ms":200000,"available":true,"plays_from_services":false,"liked":false,"album":null,"release_id":null}"#
    }

    @Test func theArtistPageHoldsTracksAlbumsAndAppearances() async throws {
        let release = #"{"id":"a1","library_id":"l1","title":"Record","artist":"Band","artist_id":"ar1","track_count":9,"duration_ms":1800000,"inferred_artist":null,"year":2001}"#
        let body = #"{"artist":{"id":"ar1","library_id":"l1","name":"Band","recording_count":12,"release_count":1,"featured_recording_count":2,"member_only":false,"provider_photo":false,"artwork_recording_id":null},"recordings":{"items":[\#(recording("r1"))],"total":12,"offset":0,"limit":50},"releases":{"items":[\#(release)],"total":1,"offset":0,"limit":50},"featured_recordings":{"items":[\#(recording("r9"))],"total":2,"offset":0,"limit":50}}"#
        let fake = FakeServer(routes: ["GET /api/v1/artists/ar1": .json(200, body)])
        let page = try await ArtistPage(api: HitoAPI(address: address, token: "session-token-1", session: fake.session)).load("ar1")
        #expect(page.name == "Band")
        #expect(page.stats == "12 tracks · 1 album in your library · 2 appearances")
        #expect(page.tracks.map(\.id) == ["r1"])
        #expect(page.albums.map(\.title) == ["Record"])
        #expect(page.appearances.map(\.id) == ["r9"])
    }

    @Test func countsReadLikeTheWeb() {
        #expect(ArtistPage.counts(tracks: 1, albums: 0, featured: 0) == "1 track")
        #expect(ArtistPage.counts(tracks: 0, albums: 0, featured: 0) == "0 tracks")
        #expect(ArtistPage.counts(tracks: 3, albums: 2, featured: 1) == "3 tracks · 2 albums in your library · 1 appearance")
    }
}
