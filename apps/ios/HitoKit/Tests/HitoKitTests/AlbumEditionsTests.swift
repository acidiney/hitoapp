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
        #expect(album.addable.map(\.id) == ["o2"])
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
}
