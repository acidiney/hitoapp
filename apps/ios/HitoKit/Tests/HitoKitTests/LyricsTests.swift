import Foundation
import Testing
@testable import HitoKit

@Suite struct LyricsTests {
    let address = try! ServerAddressInput(parsing: "http://192.168.1.20:8080").candidates[0]
    let providers = #"{"recording_id":"r1","items":[{"observation_id":"o1","installation_id":"i1","installation_revision":3,"provider_recording":{"installation_id":"i1","provider_id":"p1"},"label":"Fixture Lyrics"}]}"#

    func lyrics(_ fake: FakeServer) -> Lyrics {
        Lyrics(api: HitoAPI(address: address, token: "session-token-1", session: fake.session))
    }

    @Test func timedLinesComeFromTheFirstProvider() async throws {
        let ready = #"{"status":"ready","recording_id":"r1","observation_id":"o1","document_digest":"d","document":{"recording":{"installation_id":"i1","provider_id":"p1"},"lyrics":{"id":"l1","rendition_id":"x","lines":[{"start_ms":0,"end_ms":1000,"text":"First line"},{"start_ms":1000,"end_ms":2000,"text":"Second line"}]}}}"#
        let fake = FakeServer(routes: [
            "GET /api/v1/recordings/r1/lyrics/providers": .json(200, providers),
            "POST /api/v1/recordings/r1/lyrics": .json(200, ready),
        ])
        let found = try await lyrics(fake).load("r1")
        #expect(found == .lines([LyricLine(start: 0, end: 1, text: "First line"), LyricLine(start: 1, end: 2, text: "Second line")], from: "Fixture Lyrics"))
        let request = try #require(fake.requests.last)
        #expect(request.object?["observation_id"] as? String == "o1")
        #expect(request.object?["installation_revision"] as? Int == 3)
    }

    @Test func noProviderMeansNoLyricsWithoutAsking() async throws {
        let fake = FakeServer(routes: ["GET /api/v1/recordings/r1/lyrics/providers": .json(200, #"{"recording_id":"r1","items":[]}"#)])
        #expect(try await lyrics(fake).load("r1") == .none)
        #expect(fake.requests.count == 1)
    }

    @Test func aTrackWithNoLyricsProvidersHasNoLyrics() async throws {
        let fake = FakeServer(routes: ["GET /api/v1/recordings/r1/lyrics/providers": .json(404, #"{"error":"Resource not found"}"#)])
        #expect(try await lyrics(fake).load("r1") == .none)
    }

    @Test func aProviderWithoutTheseLyricsSaysSo() async throws {
        let fake = FakeServer(routes: [
            "GET /api/v1/recordings/r1/lyrics/providers": .json(200, providers),
            "POST /api/v1/recordings/r1/lyrics": .json(200, #"{"status":"unavailable"}"#),
        ])
        #expect(try await lyrics(fake).load("r1") == .none)
    }
}
