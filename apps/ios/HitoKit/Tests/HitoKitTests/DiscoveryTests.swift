import Foundation
import Testing
@testable import HitoKit

@Suite struct DiscoveryTests {
    let address = try! ServerAddressInput(parsing: "http://192.168.1.20:8080").candidates[0]
    let groups = #"{"items":[{"id":"g1","provider":"musicbrainz","provider_artist_id":"m1","name":{"value":"Fixture Band","recording_id":"r1","artist_id":"a1","association_id":"x1"},"library_count":1,"recording_count":3,"release_count":1},{"id":"g2","provider":"musicbrainz","provider_artist_id":"m2","name":{"value":"Other Band","recording_id":"r2","artist_id":"a2","association_id":"x2"},"library_count":1,"recording_count":9,"release_count":2}],"total":2,"offset":0,"limit":24}"#
    let recording = #"{"id":"r1","library_id":"l1","title":"Fixture Song","inherited_title":"Fixture Song","inherited_artist":"Fixture Band","artist":"Fixture Band","duration_ms":200000,"available":true,"plays_from_services":false,"liked":false,"album":null,"release_id":null}"#

    func discovery(_ fake: FakeServer) -> Discovery {
        Discovery(api: HitoAPI(address: address, token: "session-token-1", session: fake.session))
    }

    @Test func radioArtistsAreListedWithTheArtistYouOwnMostFirst() async throws {
        let fake = FakeServer(routes: ["GET /api/v1/artist-identity-groups": .json(200, groups)])
        let artists = try await discovery(fake).artists()
        #expect(artists.items.map(\.name) == ["Other Band", "Fixture Band"])
        #expect(artists.items.map(\.tracks) == [9, 3])
        #expect(artists.heroLabel == "Radio · because you own the most of this artist")
    }

    @Test func aPartialListDoesNotClaimTheArtistYouOwnMost() async throws {
        let fake = FakeServer(routes: ["GET /api/v1/artist-identity-groups": .json(200, groups.replacingOccurrences(of: #""total":2"#, with: #""total":40"#))])
        let artists = try await discovery(fake).artists()
        #expect(artists.heroLabel == "Radio · from an artist in your library")
    }

    @Test func aRadioStartsFromAnArtistAndNamesHowFarItReaches() async throws {
        let fake = FakeServer(routes: ["POST /api/v1/artist-identity-groups/g1/radio": .json(200, #"{"artist":"Fixture Band","similar_artists":4,"tracks":[\#(recording)]}"#)])
        let radio = try await discovery(fake).radio("g1")
        #expect(radio.tracks.map(\.id) == ["r1"])
        #expect(radio.reach == "Starts with 1 ready track, then 4 nearby artists in your library.")
    }

    @Test func aRadioWithNoNearbyArtistsStaysWithTheArtist() async throws {
        let fake = FakeServer(routes: ["POST /api/v1/artist-identity-groups/g1/radio": .json(200, #"{"artist":"Fixture Band","similar_artists":0,"tracks":[\#(recording)]}"#)])
        #expect(try await discovery(fake).radio("g1").reach == "Starts with 1 ready track of Fixture Band.")
    }

    @Test func theFeaturedShelfListsWhatAPluginChose() async throws {
        let fake = FakeServer(routes: ["GET /api/v1/discover/featured": .json(200, #"{"status":"available","items":[{"installation_id":"i1","installation_revision":2,"provider":"YouTube","id":"f1","title":"Owned Mix","subtitle":"Owned Band","kind":"playlist","reference":"ref-1","item_count":12,"existing_playlist_id":null}]}"#)])
        let shelf = try await discovery(fake).featured()
        #expect(shelf == .available([FeaturedItem(id: "f1", title: "Owned Mix", detail: "Owned Band · YouTube", installation: "i1", revision: 2, reference: "ref-1", playlist: nil)]))
    }

    @Test func aShelfThatIsOffSaysWhy() async throws {
        let fake = FakeServer(routes: ["GET /api/v1/discover/featured": .json(200, #"{"status":"unsupported"}"#)])
        #expect(try await discovery(fake).featured() == .off)
    }

    @Test func openingAFeaturedItemReusesItsPlaylistOrOpensOne() async throws {
        let request = UUID(uuidString: "11111111-2222-4333-8444-555555555555")!
        let item = FeaturedItem(id: "f1", title: "Owned Mix", detail: "", installation: "i1", revision: 2, reference: "ref-1", playlist: nil)
        let fake = FakeServer(routes: ["POST /api/v1/plugins/i1/playlists/open": .json(200, #"{"request_id":"11111111-2222-4333-8444-555555555555","playlist_id":"p7","reused":false}"#)])
        #expect(try await discovery(fake).open(item, request: request) == "p7")
        #expect(fake.requests.first?.object?["playlist_reference"] as? String == "ref-1")
        let known = FeaturedItem(id: "f1", title: "Owned Mix", detail: "", installation: "i1", revision: 2, reference: "ref-1", playlist: "p3")
        let none = FakeServer(routes: [:])
        #expect(try await discovery(none).open(known) == "p3")
        #expect(none.requests.isEmpty)
    }
}
