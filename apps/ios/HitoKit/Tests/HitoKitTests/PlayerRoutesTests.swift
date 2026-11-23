import Foundation
import Testing
@testable import HitoKit

@Suite struct PlayerRoutesTests {
    let address = try! ServerAddressInput(parsing: "http://192.168.1.20:8080").candidates[0]
    let playlist = Playlist(id: "p1", name: "Late night", kind: .local)

    func track(id: String = "r1", artistId: String? = "a1", releaseId: String? = "rel1", credits: [Track.Credit]? = nil) -> Track {
        Track(id: id, title: "Fixture Song", artist: "Fixture Band", album: releaseId == nil ? nil : "Fixture Album", releaseId: releaseId, durationMs: 1000, available: true, playsFromServices: false, liked: false, artistId: artistId, credits: credits)
    }

    func identity(_ fake: FakeServer) -> ArtistIdentity {
        ArtistIdentity(api: HitoAPI(address: address, token: "session-token-1", session: fake.session))
    }

    @Test func aTrackReadsItsArtistAndCreditedArtistsFromTheServer() throws {
        let json = #"{"id":"r1","library_id":"l1","title":"Duet","inherited_title":"Duet","inherited_artist":"Duo A & Duo B","artist":"Duo A & Duo B","artist_id":"a1","duration_ms":1000,"available":true,"plays_from_services":false,"liked":false,"album":null,"release_id":null,"credited_artists":[{"id":"a1","name":"Duo A","role":"main","join":" & "},{"id":"a2","name":"Duo B","role":"featured"}]}"#
        let read = try JSONDecoder().decode(Track.self, from: Data(json.utf8))
        #expect(read.artistId == "a1")
        #expect(read.credits == [Track.Credit(id: "a1", name: "Duo A"), Track.Credit(id: "a2", name: "Duo B")])
    }

    @Test func aLibraryTrackGoesToItsArtistAndAlbum() {
        let routes = PlayerRoutes(track: track())
        #expect(routes.artists == [.artist(Artist(id: "a1", name: "Fixture Band", trackCount: 0, albumCount: 0))])
        #expect(routes.album == .album(Album(id: "rel1", title: "Fixture Album", artist: "Fixture Band", trackCount: 0, year: nil, durationMs: nil)))
        #expect(routes.playingFrom == routes.album)
        #expect(!routes.needsIdentity)
    }

    @Test func aTrackCreditingSeveralArtistsOffersEachOnceInCreditOrder() {
        let credits = [Track.Credit(id: "a1", name: "Duo A"), Track.Credit(id: "a2", name: "Duo B"), Track.Credit(id: "a1", name: "Duo A")]
        let routes = PlayerRoutes(track: track(credits: credits))
        #expect(routes.artists.map(\.title) == ["Duo A", "Duo B"])
        #expect(routes.artists.last == .artist(Artist(id: "a2", name: "Duo B", trackCount: 0, albumCount: 0)))
    }

    @Test func playingFromAPlaylistOpensThePlaylist() {
        let routes = PlayerRoutes(track: track(), playlist: playlist)
        #expect(routes.playingFrom == .playlist(playlist))
        #expect(routes.album?.title == "Fixture Album")
    }

    @Test func aTrackWithoutAnAlbumHidesGoToAlbum() {
        let routes = PlayerRoutes(track: track(releaseId: nil))
        #expect(routes.album == nil)
        #expect(routes.playingFrom == nil)
    }

    @Test func aTrackWithoutALibraryArtistAsksForTheServicesArtistAndHidesTheOptionUntilOneIsFound() {
        let routes = PlayerRoutes(track: track(artistId: nil))
        #expect(routes.artists.isEmpty)
        #expect(routes.needsIdentity)
        let group = ArtistGroup(id: "g1", name: "Fixture Band", recordingId: "r1", associationId: "x1", recordingCount: 0, releaseCount: 0)
        let found = PlayerRoutes(track: track(artistId: nil), identity: group)
        #expect(found.artists == [.artistGroup(group)])
        #expect(!found.needsIdentity)
    }

    @Test func aPlaylistEntryWithoutARecordingNeverAsksTheServer() {
        let routes = PlayerRoutes(track: track(id: "entry:e1", artistId: nil, releaseId: nil))
        #expect(routes.artists.isEmpty)
        #expect(routes.album == nil)
        #expect(!routes.needsIdentity)
    }

    @Test func theServicesChosenArtistOpensItsDiscography() async throws {
        let fake = FakeServer(routes: ["GET /api/v1/recordings/r1/artist-identity": .json(200, #"{"recording_id":"r1","revision":3,"state":{"status":"selected","group_id":"g1","association_id":"x1"}}"#)])
        let group = try await identity(fake).group(for: track(artistId: nil))
        #expect(group == ArtistGroup(id: "g1", name: "Fixture Band", recordingId: "r1", associationId: "x1", recordingCount: 0, releaseCount: 0))
        #expect(fake.requests.first?.authorization == "Bearer session-token-1")
    }

    @Test func anArtistNotYetChosenOrWaitingForReviewHasNoDiscography() async throws {
        let unselected = FakeServer(routes: ["GET /api/v1/recordings/r1/artist-identity": .json(200, #"{"recording_id":"r1","revision":0,"state":{"status":"unselected"}}"#)])
        #expect(try await identity(unselected).group(for: track(artistId: nil)) == nil)
        let review = FakeServer(routes: ["GET /api/v1/recordings/r1/artist-identity": .json(200, #"{"recording_id":"r1","revision":1,"state":{"status":"needs_review","group_id":"g1","association_id":"x1"}}"#)])
        #expect(try await identity(review).group(for: track(artistId: nil)) == nil)
    }
}
