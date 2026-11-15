import Foundation
import Testing
@testable import HitoKit

@Suite struct TrackMetadataTests {
    let address = try! ServerAddressInput(parsing: "http://192.168.1.20:8080").candidates[0]

    func metadata(_ fake: FakeServer) -> TrackMetadata {
        TrackMetadata(api: HitoAPI(address: address, token: "session-token-1", session: fake.session), device: "device-1")
    }

    let body = #"{"recording_id":"r1","title":{"value":"My Song","inherited":"Song","personal":"My Song","operation_id":"o1"},"artist":{"value":"Band","inherited":"Band","personal":null,"operation_id":null},"musicbrainz_recording_id":"mb-1","release_facts":{"year":1999,"track_total":12,"label":"Label"},"audio_format":{"format":"flac","bit_depth":16,"sample_rate_hz":44100,"size_bytes":20000000},"last_played":null,"appears_in":[],"credited_artists":[],"observations":{"items":[],"next_cursor":null},"selected_observation":{"album":"Record","duration_ms":200000}}"#

    @Test func detailsSayWhichValuesAreYours() async throws {
        let fake = FakeServer(routes: ["GET /api/v1/recordings/r1/metadata": .json(200, body)])
        let loaded = try await metadata(fake).load("r1")
        #expect(loaded.title == .init(value: "My Song", source: "Song", personal: "My Song"))
        #expect(loaded.title.note(field: "title") == "Personal title · Source title: Song")
        #expect(loaded.artist.note(field: "artist") == "Following source metadata · Source artist: Band")
        #expect(loaded.facts == ["Record", "1999", "Label", "FLAC · 16-bit · 44.1 kHz"])
        #expect(loaded.musicBrainz == "mb-1")
    }

    @Test func savingAPersonalValueAndGoingBackAreSyncChanges() async throws {
        let fake = FakeServer(routes: ["POST /api/v1/sync": .acknowledging])
        try await metadata(fake).save("r1", field: .title, personal: "Mine")
        try await metadata(fake).save("r1", field: .artist, personal: nil)
        let actions = fake.requests.compactMap { (($0.object?["operations"] as? [[String: Any]])?.first?["action"]) as? [String: String] }
        #expect(actions == [["kind": "recording_title_set", "recording_id": "r1", "title": "Mine"], ["kind": "recording_artist_inherit", "recording_id": "r1"]])
        #expect(fake.requests.first?.object?["device_id"] as? String == "device-1")
    }
}
