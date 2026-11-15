import Foundation
import Testing
@testable import HitoKit

@Suite struct AlbumEditionsTests {
    let address = try! ServerAddressInput(parsing: "http://192.168.1.20:8080").candidates[0]
    let saved = SavedEdition(recordingId: "r1", associationId: "a1", releaseGroupId: "g1", editionId: "e1")

    func recording(_ id: String, _ title: String) -> String {
        #"{"id":"\#(id)","library_id":"l1","title":"\#(title)","inherited_title":"\#(title)","inherited_artist":"Fixture Band","artist":"Fixture Band","duration_ms":200000,"available":true,"plays_from_services":false,"liked":false,"album":null,"release_id":null}"#
    }

    func list(adoption: String = "null") -> String {
        let tracks = [
            #"{"id":"o1","position":1,"number":"1","kind":"audio","title":"First","duration_ms":200000,"recording_id":"m1"}"#,
            #"{"id":"o2","position":2,"number":"2","kind":"audio","title":"Second","duration_ms":180000,"recording_id":"m2"}"#,
            #"{"id":"o3","position":3,"number":"3","kind":"audio","title":"Third","duration_ms":null,"recording_id":null}"#,
            #"{"id":"o4","position":4,"number":"4","kind":"audio","title":"Fourth","duration_ms":null,"recording_id":"m4"}"#,
            #"{"id":"o0","position":0,"number":null,"kind":"pregap","title":"Hidden","duration_ms":null,"recording_id":null}"#,
        ].joined(separator: ",")
        return #"{"status":"available","association_id":"a1","edition_digest":"d","is_album_release":true,"edition":{"id":"e1","release_group_id":"g1","title":"Standard","media":[{"position":1,"tracks":[\#(tracks)]}]},"stats":{"audio_tracks":4,"total_duration_ms":null},"matches":[{"occurrence_id":"o1","recording":\#(recording("r1", "First"))}],"library_only":[],"attached_occurrences":["o4"],"adoption":\#(adoption)}"#
    }

    func editions(_ fake: FakeServer) -> AlbumEditions {
        AlbumEditions(api: HitoAPI(address: address, token: "session-token-1", session: fake.session))
    }

    @Test func theSavedEditionSaysHowMuchOfTheAlbumYouHave() async throws {
        let fake = FakeServer(routes: ["POST /api/v1/albums/rel1/edition/track-list": .json(200, list())])
        guard case let .available(album) = try await editions(fake).read("rel1") else { Issue.record("not available"); return }
        #expect(album.editionName == "Standard")
        #expect(album.tracks.map(\.title) == ["First", "Second", "Third", "Fourth"])
        #expect(album.summary == "1 of 4 in your library")
        #expect(album.tracks.first?.owned?.id == "r1")
        #expect(album.addable.map(\.id) == ["o2", "o3"])
        #expect(album.tracks.last?.note == "Already attached to another recording")
    }

    @Test func anEditionThatCantBeReadSaysWhy() async throws {
        let fake = FakeServer(routes: ["POST /api/v1/albums/rel1/edition/track-list": .json(200, #"{"status":"disabled"}"#)])
        #expect(try await editions(fake).read("rel1") == .unavailable("Track details from plugins are off in your settings."))
    }

    @Test func aMissingTrackIsFoundThroughTheEdition() async throws {
        let fake = FakeServer(routes: [
            "POST /api/v1/albums/rel1/edition/track-list": .json(200, list()),
            "POST /api/v1/albums/rel1/edition-track-audio": .json(200, #"{"status":"saved","recording":\#(recording("r2", "Second"))}"#),
        ])
        guard case let .available(album) = try await editions(fake).read("rel1") else { return }
        let result = try await editions(fake).add(album.addable[0], of: album, release: "rel1", saved: saved)
        #expect(result == .saved(try JSONDecoder().decode(Track.self, from: Data(recording("r2", "Second").utf8))))
        #expect(fake.requests.last?.json == ["recording_id": "r1", "association_id": "a1", "release_group_id": "g1", "edition_id": "e1", "occurrence_id": "o2"])
    }

    @Test func aPluginAlbumAdoptsTheTrackDirectly() async throws {
        let fake = FakeServer(routes: [
            "POST /api/v1/albums/rel1/edition/track-list": .json(200, list(adoption: #"{"installation_id":"i1","installation_revision":3}"#)),
            "POST /api/v1/plugins/i1/recordings": .json(200, recording("r2", "Second")),
        ])
        guard case let .available(album) = try await editions(fake).read("rel1") else { return }
        #expect(album.addable.map(\.id) == ["o2", "o3"])
        #expect(album.tracks[2].note == "Not in your library · No provider identity for this track")
        _ = try await editions(fake).add(album.addable[0], of: album, release: "rel1", saved: saved)
        #expect(fake.requests.last?.object?["provider_recording_id"] as? String == "m2")
    }

    @Test func aTrackWithNoAudioSaysSo() async throws {
        let fake = FakeServer(routes: [
            "POST /api/v1/albums/rel1/edition/track-list": .json(200, list()),
            "POST /api/v1/albums/rel1/edition-track-audio": .json(200, #"{"status":"no_match"}"#),
        ])
        guard case let .available(album) = try await editions(fake).read("rel1") else { return }
        #expect(try await editions(fake).add(album.addable[1], of: album, release: "rel1", saved: saved) == .missing("No audio found"))
    }

    @Test func aReleaseCarriesItsSavedEdition() throws {
        let json = #"{"release":{"id":"rel1","title":"Album","artist":"Band","year":null,"track_count":1,"duration_ms":1000,"library_id":"l1"},"tracks":{"items":[],"total":0,"offset":0,"limit":200},"catalog_anchor":null,"saved_edition":{"revision":2,"state":{"status":"ready","edition":{"recording_id":"r1","association_id":"a1","release_group_id":"g1","edition_id":"e1"}}}}"#
        let detail = try JSONDecoder().decode(AlbumDetail.self, from: Data(json.utf8))
        #expect(detail.savedEdition == saved)
    }

    let anchor = CatalogAnchor(recordingId: "r1", associationId: "a1")

    @Test func aReleaseSaysWhetherAnEditionCanBeChosen() throws {
        func detail(_ anchor: String, _ state: String) throws -> AlbumDetail {
            let json = #"{"release":{"id":"rel1","title":"Album","artist":"Band","year":null,"track_count":1,"duration_ms":1000,"library_id":"l1"},"tracks":{"items":[],"total":0,"offset":0,"limit":200},"catalog_anchor":\#(anchor),"saved_edition":{"revision":3,"state":\#(state)}}"#
            return try JSONDecoder().decode(AlbumDetail.self, from: Data(json.utf8))
        }
        let unset = try detail(#"{"recording_id":"r1","association_id":"a1"}"#, #"{"status":"unset"}"#)
        #expect(unset.anchor == anchor)
        #expect(unset.choiceRevision == 3)
        #expect(unset.needsChoice == false)
        let stale = try detail("null", #"{"status":"needs_choice"}"#)
        #expect(stale.anchor == nil)
        #expect(stale.needsChoice)
    }

    func discography(_ titles: [(String, String, String)]) -> String {
        let items = titles.map { #"{"id":"\#($0.0)","title":"\#($0.1)","artist_credit":null,"kind":"\#($0.2)","secondary_types":[],"first_release_date":null,"revision":null}"# }.joined(separator: ",")
        return #"{"status":"available","association_id":"a1","continuation":null,"page":{"artist_id":"x","snapshot":null,"items":[\#(items)],"total_count":\#(titles.count),"next_cursor":null}}"#
    }

    let editionPage = #"{"status":"available","association_id":"a1","continuation":null,"page":{"release_group_id":"g2","snapshot":null,"items":[{"id":"e1","title":"Album","artist_credit":null,"disambiguation":"deluxe","release_date":"2019-05","country":"GB","revision":null},{"id":"e2","title":"Album","artist_credit":null,"disambiguation":null,"release_date":null,"country":null,"revision":null}],"total_count":2,"next_cursor":null}}"#

    @Test func theAlbumsEditionsAreFoundThroughItsArtist() async throws {
        let fake = FakeServer(routes: [
            "POST /api/v1/recordings/r1/artist-associations/a1/discography": .json(200, discography([("g1", "Album Live", "album"), ("g2", "  album ", "album"), ("g3", "Album", "single")])),
            "POST /api/v1/recordings/r1/artist-associations/a1/release-group-editions": .json(200, editionPage),
        ])
        let options = try await editions(fake).options(for: "Album", anchor: anchor)
        #expect(options == [
            EditionOption(id: "e1", releaseGroupId: "g2", name: "Deluxe", detail: "2019 · GB"),
            EditionOption(id: "e2", releaseGroupId: "g2", name: "Album", detail: nil),
        ])
        #expect(options.map(\.label) == ["Deluxe · 2019 · GB", "Album"])
        #expect(fake.requests.first?.object?["limit"] as? Int == 20)
        #expect(fake.requests.first?.object?["continuation"] is NSNull)
        #expect(fake.requests.last?.object?["release_group_id"] as? String == "g2")
    }

    @Test func anAlbumTheArtistDoesntListHasNoEditions() async throws {
        let fake = FakeServer(routes: ["POST /api/v1/recordings/r1/artist-associations/a1/discography": .json(200, discography([("g1", "Other", "album")]))])
        #expect(try await editions(fake).options(for: "Album", anchor: anchor).isEmpty)
        let off = FakeServer(routes: ["POST /api/v1/recordings/r1/artist-associations/a1/discography": .json(200, #"{"status":"disabled"}"#)])
        await #expect(throws: EditionOption.Unavailable(reason: "Track details from plugins are off in your settings.")) {
            try await editions(off).options(for: "Album", anchor: anchor)
        }
    }

    let deluxe = EditionOption(id: "e1", releaseGroupId: "g1", name: "Deluxe", detail: nil)

    @Test func anEditionIsComparedBeforeItIsSaved() async throws {
        let fake = FakeServer(routes: ["POST /api/v1/albums/rel1/edition-track-list": .json(200, list())])
        guard case let .available(album) = try await editions(fake).preview(deluxe, release: "rel1", anchor: anchor) else { Issue.record("not available"); return }
        #expect(album.summary == "1 of 4 in your library")
        #expect(fake.requests.first?.json == ["recording_id": "r1", "association_id": "a1", "release_group_id": "g1", "edition_id": "e1"])
    }

    @Test func choosingAnEditionSavesItOnceWithTheRevision() async throws {
        let request = UUID(uuidString: "11111111-2222-4333-8444-555555555555")!
        let fake = FakeServer(routes: ["PUT /api/v1/albums/rel1/edition": .json(200, #"{"request_id":"11111111-2222-4333-8444-555555555555","release_id":"rel1","revision":4,"edition":{"recording_id":"r1","association_id":"a1","release_group_id":"g1","edition_id":"e1"}}"#)])
        let saved = try await editions(fake).choose(deluxe, release: "rel1", anchor: anchor, revision: 3, request: request)
        #expect(saved == 4)
        let body = try #require(fake.requests.first?.object)
        #expect(body["request_id"] as? String == "11111111-2222-4333-8444-555555555555")
        #expect(body["expected_revision"] as? Int == 3)
        #expect(body["edition_id"] as? String == "e1")
        let stale = FakeServer(routes: ["PUT /api/v1/albums/rel1/edition": .json(409, #"{"error":"changed"}"#)])
        await #expect(throws: EditionOption.Changed()) {
            try await editions(stale).choose(deluxe, release: "rel1", anchor: anchor, revision: 3, request: request)
        }
    }

    @Test func forgettingAnEditionClearsIt() async throws {
        let request = UUID(uuidString: "11111111-2222-4333-8444-555555555555")!
        let fake = FakeServer(routes: ["POST /api/v1/albums/rel1/edition/clear": .json(200, #"{"request_id":"11111111-2222-4333-8444-555555555555","release_id":"rel1","revision":5,"edition":null}"#)])
        #expect(try await editions(fake).forget(release: "rel1", revision: 4, request: request) == 5)
        #expect(fake.requests.first?.object?["expected_revision"] as? Int == 4)
    }
}
