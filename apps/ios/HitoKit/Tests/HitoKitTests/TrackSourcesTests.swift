import Foundation
import Testing
@testable import HitoKit

@Suite struct TrackSourcesTests {
    let address = try! ServerAddressInput(parsing: "http://192.168.1.20:8080").candidates[0]

    func sources(_ fake: FakeServer) -> TrackSources {
        TrackSources(api: HitoAPI(address: address, token: "session-token-1", session: fake.session))
    }

    let choice = #"{"recording_id":"r1","mode":"manual","revision":3,"sources":[{"selectable":true,"selected":true,"source":{"id":"s1","recording_id":"r1","provider":"local","label":"Fixture 440 Hz.wav","provider_label":null,"ready":true,"can_prepare":false,"match_kind":"verified","device_id":null}},{"selectable":true,"selected":false,"source":{"id":"s2","recording_id":"r1","provider":"youtube","label":"Band / Song","provider_label":"YouTube","ready":false,"can_prepare":true,"match_kind":"best_guess","device_id":null}}]}"#

    @Test func theChoiceListsEachSourceLikeTheWeb() async throws {
        let fake = FakeServer(routes: ["GET /api/v1/recordings/r1/source-choice": .json(200, choice)])
        let loaded = try await sources(fake).load("r1")
        #expect(loaded.mode == .manual)
        #expect(loaded.revision == 3)
        #expect(loaded.entries.map(\.primary) == ["Fixture 440 Hz.wav", "YouTube"])
        #expect(loaded.entries.map(\.secondary) == ["Music folder", "Band · Song"])
        #expect(loaded.entries.map(\.state) == ["Ready", "Not prepared"])
        #expect(loaded.entries.map(\.pinned) == [true, false])
        #expect(loaded.entries.map(\.bestGuess) == [false, true])
        #expect(loaded.entries.map(\.canPrepare) == [false, true])
        #expect(loaded.modeLabel == "Manual source selection")
    }

    @Test func choosingASourceSendsItWithAFreshRequest() async throws {
        let fake = FakeServer(routes: ["POST /api/v1/recordings/r1/source-choice": .json(200, #"{"recording_id":"r1","request_id":"x","revision":4,"source_id":"s2"}"#)])
        #expect(try await sources(fake).choose("r1", source: "s2") == 4)
        let body = try #require(fake.requests.first?.json)
        #expect(body["source_id"] == "s2")
        #expect(UUID(uuidString: body["request_id"] ?? "") != nil)
    }

    @Test func returningToAutomaticNamesTheRevisionItSaw() async throws {
        let fake = FakeServer(routes: ["POST /api/v1/recordings/r1/source-choice/reset": .json(200, #"{"recording_id":"r1","request_id":"x","revision":5}"#)])
        #expect(try await sources(fake).reset("r1", revision: 4) == 5)
        #expect(fake.requests.first?.object?["expected_revision"] as? Int == 4)
    }

    @Test func aChangedChoiceAsksToLookAgain() async {
        let fake = FakeServer(routes: ["POST /api/v1/recordings/r1/source-choice/reset": .json(409, #"{"error":"Conflict"}"#)])
        await #expect(throws: TrackSources.Problem.changed) { try await sources(fake).reset("r1", revision: 4) }
    }
    @Test func aTrackWithNoSourcesHasNothingToChoose() async throws {
        let fake = FakeServer(routes: ["GET /api/v1/recordings/r1/source-choice": .json(404, #"{"error":"Resource not found"}"#)])
        let loaded = try await sources(fake).load("r1")
        #expect(loaded.entries.isEmpty)
        #expect(loaded.mode == .automatic)
    }
}
