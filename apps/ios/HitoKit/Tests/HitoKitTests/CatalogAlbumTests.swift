import Foundation
import Testing
@testable import HitoKit

@Suite struct CatalogAlbumTests {
    let address = try! ServerAddressInput(parsing: "http://192.168.1.20:8080").candidates[0]
    let anchor = CatalogAnchor(recordingId: "r1", associationId: "x1")
    let association = "/api/v1/recordings/r1/artist-associations/x1"
    let recording = #"{"id":"r9","library_id":"l1","title":"Second Song","inherited_title":"Second Song","inherited_artist":"Fixture Band","artist":"Fixture Band","duration_ms":200000,"available":true,"plays_from_services":false,"liked":false}"#

    func editions(_ fake: FakeServer) -> AlbumEditions {
        AlbumEditions(api: HitoAPI(address: address, token: "session-token-1", session: fake.session))
    }

    var trackList: String {
        #"{"status":"available","association_id":"x1","edition_digest":"d1","stats":{"audio_tracks":2,"total_duration_ms":400000},"edition":{"id":"e1","release_group_id":"rg1","title":"First Album","media":[{"tracks":[{"id":"o1","number":"1","kind":"audio","title":"First Song","duration_ms":200000,"recording_id":"p1"},{"id":"o2","number":"2","kind":"audio","title":"Second Song","duration_ms":200000,"recording_id":"p2"}]}]},"linked":[{"occurrence_id":"o2","recording":\#(recording)}]}"#
    }

    @Test func aReleaseGroupListsItsEditionsWithYearAndCountry() async throws {
        let page = #"{"status":"available","association_id":"x1","continuation":null,"page":{"items":[{"id":"e1","title":"First Album","disambiguation":"deluxe","release_date":"1999-05-01","country":"GB"},{"id":"e2","title":"First Album","disambiguation":null,"release_date":null,"country":null}]}}"#
        let fake = FakeServer(routes: ["POST \(association)/release-group-editions": .json(200, page)])
        let read = try await editions(fake).editions(of: "rg1", anchor: anchor)
        #expect(read == [
            EditionOption(id: "e1", releaseGroupId: "rg1", name: "Deluxe", detail: "1999 · GB"),
            EditionOption(id: "e2", releaseGroupId: "rg1", name: "First Album", detail: nil),
        ])
        #expect(fake.requests.last?.object?["release_group_id"] as? String == "rg1")
    }

    @Test func aCatalogEditionShowsWhichTracksAreAlreadyInTheLibrary() async throws {
        let fake = FakeServer(routes: ["POST \(association)/edition-track-list": .json(200, trackList)])
        let option = EditionOption(id: "e1", releaseGroupId: "rg1", name: "First Album", detail: nil)
        guard case let .available(album) = try await editions(fake).catalog(option, anchor: anchor) else {
            Issue.record("expected an available edition")
            return
        }
        #expect(album.tracks.map(\.title) == ["First Song", "Second Song"])
        #expect(album.tracks.map { $0.owned?.id } == [nil, "r9"])
        #expect(album.summary == "1 of 2 in your library")
        let body = fake.requests.last?.object
        #expect(body?["release_group_id"] as? String == "rg1")
        #expect(body?["edition_id"] as? String == "e1")
    }

    @Test func aMissingCatalogTrackIsFoundThroughTheArtist() async throws {
        let fake = FakeServer(routes: [
            "POST \(association)/edition-track-list": .json(200, trackList),
            "POST \(association)/edition-track-audio": .json(200, #"{"status":"saved","recording":\#(recording)}"#),
        ])
        let option = EditionOption(id: "e1", releaseGroupId: "rg1", name: "First Album", detail: nil)
        guard case let .available(album) = try await editions(fake).catalog(option, anchor: anchor) else {
            Issue.record("expected an available edition")
            return
        }
        let found = try await editions(fake).find(album.tracks[0], of: album, anchor: anchor)
        #expect(found == .saved(try JSONDecoder().decode(Track.self, from: Data(recording.utf8))))
        let body = fake.requests.last?.object
        #expect(body?["release_group_id"] as? String == "rg1")
        #expect(body?["edition_id"] as? String == "e1")
        #expect(body?["occurrence_id"] as? String == "o1")
    }

    @Test func aCatalogTrackWithNoAudioSaysWhy() async throws {
        let fake = FakeServer(routes: ["POST \(association)/edition-track-audio": .json(200, #"{"status":"no_providers"}"#)])
        let album = CompleteAlbum(editionName: "First Album", editionId: "e1", associationId: "x1", releaseGroupId: "rg1", tracks: [], adoption: nil)
        let track = EditionTrack(id: "o1", number: "1", title: "First Song", durationMs: nil, providerRecording: nil, owned: nil, attached: false)
        #expect(try await editions(fake).find(track, of: album, anchor: anchor) == .missing("No connected plugin can find audio"))
    }
}
