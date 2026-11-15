import Foundation
import Testing
@testable import HitoKit

@Suite struct MetadataHistoryTests {
    let address = try! ServerAddressInput(parsing: "http://192.168.1.20:8080").candidates[0]

    func history(_ fake: FakeServer) -> MetadataHistory {
        MetadataHistory(api: HitoAPI(address: address, token: "session-token-1", session: fake.session))
    }

    let file = #"{"id":"o1","source_id":null,"release_id":null,"provider":"local","origin":"file_inspection","observed_at":1700000000,"inspection_version":1,"plugin_installation_id":null,"provider_recording_id":null,"provider_revision":null,"title":"Owned title","artist":"Owned artist","album":null,"album_artist":null,"duration_ms":1000,"disc_number":null,"track_number":null,"tags":{"title":{"value":"Owned title","location":{"kind":"format"}},"artist":{"value":"Owned artist","location":{"kind":"audio_stream","index":0}},"album":null,"album_artist":null,"disc":null,"track":null,"track_total":null,"audio":{"format":"WAV","bit_depth":16,"sample_rate_hz":16000},"genre":{"value":"Owned genre","location":{"kind":"format"}},"musicbrainz_artist_tags":[]}}"#
    let provider = #"{"id":"o2","source_id":null,"release_id":null,"provider":"Owned fake","origin":"provider_response","observed_at":null,"inspection_version":1,"plugin_installation_id":"i1","provider_recording_id":"p-9","provider_revision":"r3","title":"Owned title","artist":"Owned artist","album":"Owned album","album_artist":null,"duration_ms":1000,"disc_number":null,"track_number":null,"tags":null}"#

    @Test func observationsReadLikeTheWeb() async throws {
        let body = #"{"recording_id":"t1","title":{"value":"Owned title"},"artist":{"value":"Owned artist"},"selected_observation":\#(provider),"observations":{"items":[\#(file),\#(provider)],"next_before":1699999000}}"#
        let fake = FakeServer(routes: ["GET /api/v1/recordings/t1/metadata": .json(200, body)])
        let page = try await history(fake).page("t1", before: nil)
        #expect(page.observations.map(\.id) == ["o1", "o2"])
        #expect(page.selected == "o2")
        #expect(page.next == 1699999000)
        let local = page.observations[0]
        #expect(local.heading == "Metadata read from local file")
        #expect(local.summary == "Owned title · Owned artist · Unknown album")
        #expect(local.when == Date(timeIntervalSince1970: 1700000000))
        #expect(local.tags.map(\.label) == ["Title", "Artist", "Album", "Album artist", "Disc", "Track"])
        #expect(local.tags.map(\.value) == ["Owned title", "Owned artist", nil, nil, nil, nil])
        #expect(local.tags.map(\.place) == ["File tags", "Audio stream 1", nil, nil, nil, nil])
        let remote = page.observations[1]
        #expect(remote.heading == "Metadata read from Owned fake")
        #expect(remote.when == nil)
        #expect(remote.origin == .provider(recording: "p-9", revision: "r3"))
    }

    @Test func olderObservationsAskForTheCursor() async throws {
        let fake = FakeServer(routes: ["GET /api/v1/recordings/t1/metadata": .json(200, #"{"recording_id":"t1","selected_observation":null,"observations":{"items":[],"next_before":null}}"#)])
        let page = try await history(fake).page("t1", before: 1699999000)
        #expect(page.observations.isEmpty)
        #expect(page.next == nil)
        #expect(fake.requests.first?.query == "before=1699999000")
    }
}
