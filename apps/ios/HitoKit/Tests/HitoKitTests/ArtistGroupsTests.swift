import Foundation
import Testing
@testable import HitoKit

@Suite struct ArtistGroupsTests {
    let address = try! ServerAddressInput(parsing: "http://192.168.1.20:8080").candidates[0]
    let recording = #"{"id":"r1","library_id":"l1","title":"Fixture Song","inherited_title":"Fixture Song","inherited_artist":"Fixture Band","artist":"Fixture Band","duration_ms":200000,"available":true,"plays_from_services":false,"liked":false,"album":"Owned Album","release_id":"rel1"}"#
    let release = #"{"id":"rel1","library_id":"l1","title":"Owned Album","artist":"Fixture Band","track_count":9,"year":2001,"duration_ms":1800000}"#

    var detail: String {
        #"{"group":{"id":"g1","provider":{"kind":"builtin_musicbrainz"},"provider_artist_id":"m1","name":{"value":"Fixture Band","recording_id":"r1","artist_id":"a1","association_id":"x1"},"library_count":1,"recording_count":3,"release_count":1},"libraries":{"items":[],"total":0,"offset":0,"limit":20},"recordings":{"items":[{"recording":\#(recording),"artist_id":"a1","association_id":"x1","choice_revision":1}],"total":3,"offset":0,"limit":20},"releases":{"items":[\#(release)],"total":1,"offset":0,"limit":20}}"#
    }

    func discography(_ items: String, continuation: String = "null") -> String {
        #"{"status":"available","association_id":"x1","continuation":\#(continuation),"page":{"artist_id":"m1","items":[\#(items)]}}"#
    }

    func groups(_ fake: FakeServer) -> ArtistGroups {
        ArtistGroups(api: HitoAPI(address: address, token: "session-token-1", session: fake.session))
    }

    @Test func aGroupReadsItsNameCountsTracksAndLibraryAlbums() async throws {
        let fake = FakeServer(routes: ["GET /api/v1/artist-identity-groups/g1": .json(200, detail)])
        let read = try await groups(fake).detail("g1")
        #expect(read.group.name == "Fixture Band")
        #expect(read.group.recordingId == "r1")
        #expect(read.group.associationId == "x1")
        #expect(read.tracks.map(\.id) == ["r1"])
        #expect(read.tracksTotal == 3)
        #expect(read.albums.map(\.id) == ["rel1"])
        #expect(read.stats == "3 tracks · 1 album in your library")
    }

    @Test func theDiscographyListsReleaseGroupsWithTheirYearsAndContinuation() async throws {
        let items = #"{"id":"rg1","title":"First Album","kind":"album","first_release_date":"1999-05-01","secondary_types":[]},{"id":"rg2","title":"A Single","kind":"single","first_release_date":null,"secondary_types":[]}"#
        let fake = FakeServer(routes: ["POST /api/v1/recordings/r1/artist-associations/x1/discography": .json(200, discography(items, continuation: #""next""#))])
        let group = try await groups(FakeServer(routes: ["GET /api/v1/artist-identity-groups/g1": .json(200, detail)])).detail("g1").group
        let read = try await groups(fake).discography(group, continuation: nil)
        #expect(read == .available([
            ReleaseGroup(id: "rg1", title: "First Album", kind: .album, year: "1999"),
            ReleaseGroup(id: "rg2", title: "A Single", kind: .single, year: nil),
        ], continuation: "next"))
        let body = fake.requests.last?.object
        #expect(body?["limit"] as? Int == 50)
        #expect(body?["continuation"] == nil)
    }

    @Test func aFollowingDiscographyPageSendsItsContinuation() async throws {
        let fake = FakeServer(routes: ["POST /api/v1/recordings/r1/artist-associations/x1/discography": .json(200, discography(""))])
        let group = ArtistGroup(id: "g1", name: "Fixture Band", recordingId: "r1", associationId: "x1", recordingCount: 3, releaseCount: 1)
        _ = try await groups(fake).discography(group, continuation: "next")
        let body = fake.requests.last?.object
        #expect(body?["limit"] as? Int == 50)
        #expect(body?["continuation"] as? String == "next")
    }

    @Test func aDiscographyThatIsOffOrUnavailableSaysWhy() async throws {
        let group = ArtistGroup(id: "g1", name: "Fixture Band", recordingId: "r1", associationId: "x1", recordingCount: 3, releaseCount: 1)
        for (status, reason) in [
            ("disabled", "Track details from plugins are off in your settings."),
            ("unsupported", "This artist's catalog can't list releases."),
            ("unavailable", "Couldn't load the discography. Your own albums are still listed."),
        ] {
            let fake = FakeServer(routes: ["POST /api/v1/recordings/r1/artist-associations/x1/discography": .json(200, #"{"status":"\#(status)"}"#)])
            #expect(try await groups(fake).discography(group, continuation: nil) == .unavailable(reason))
        }
    }

    @Test func releaseGroupsAlreadyInTheLibraryOpenTheirAlbum() async throws {
        let fake = FakeServer(routes: ["POST /api/v1/recordings/r1/artist-associations/x1/discography/library": .json(200, #"{"association_id":"x1","groups":[{"release_group_id":"rg1","albums":[{"release_id":"rel1","edition_id":"e1","track_count":9,"playable_count":7}]},{"release_group_id":"rg2","albums":[]}]}"#)])
        let group = ArtistGroup(id: "g1", name: "Fixture Band", recordingId: "r1", associationId: "x1", recordingCount: 3, releaseCount: 1)
        let owned = try await groups(fake).library(group, releaseGroups: ["rg1", "rg2"])
        #expect(owned == ["rg1": "rel1"])
        #expect(fake.requests.last?.object?["release_group_ids"] as? [String] == ["rg1", "rg2"])
    }

    @Test func releasesAreSectionedLikeTheWeb() {
        let items = [
            ReleaseGroup(id: "1", title: "S", kind: .single, year: nil),
            ReleaseGroup(id: "2", title: "A", kind: .album, year: nil),
            ReleaseGroup(id: "3", title: "X", kind: nil, year: nil),
            ReleaseGroup(id: "4", title: "E", kind: .ep, year: nil),
        ]
        let sections = ReleaseGroup.sections(items)
        #expect(sections.map(\.title) == ["Albums", "EPs", "Singles", "Other"])
        #expect(sections.map { $0.items.map(\.id) } == [["2"], ["4"], ["1"], ["3"]])
    }

    @Test func chosenArtistsArePagedLikeTheLibrary() async throws {
        let page = #"{"items":[{"id":"g1","provider":{"kind":"builtin_musicbrainz"},"provider_artist_id":"m1","name":{"value":"Fixture Band","recording_id":"r1","artist_id":"a1","association_id":"x1"},"library_count":1,"recording_count":3,"release_count":1}],"total":41,"offset":40,"limit":40}"#
        let fake = FakeServer(routes: ["GET /api/v1/artist-identity-groups": .json(200, page)])
        let read = try await groups(fake).chosen(offset: 40)
        #expect(read.items == [ArtistGroup(id: "g1", name: "Fixture Band", recordingId: "r1", associationId: "x1", recordingCount: 3, releaseCount: 1)])
        #expect(read.total == 41)
        #expect(fake.requests.last?.query == "offset=40&limit=50")
    }

    @Test func anArtistFindsTheIdentityGroupsThatShareItsName() async throws {
        let page = #"{"items":[{"id":"g1","provider":{"kind":"builtin_musicbrainz"},"provider_artist_id":"m1","name":{"value":"Fixture Band","recording_id":"r1","artist_id":"a1","association_id":"x1"},"library_count":1,"recording_count":3,"release_count":1},{"id":"g2","provider":{"kind":"builtin_musicbrainz"},"provider_artist_id":"m2","name":{"value":"Fixture Band Tribute","recording_id":"r2","artist_id":"a2","association_id":"x2"},"library_count":1,"recording_count":1,"release_count":0}],"total":2,"offset":0,"limit":20}"#
        let fake = FakeServer(routes: ["GET /api/v1/artist-identity-groups": .json(200, page)])
        let found = try await groups(fake).named("fixture band")
        #expect(found.map(\.id) == ["g1"])
        #expect(fake.requests.last?.query == "q=fixture%20band&limit=20")
    }
}
