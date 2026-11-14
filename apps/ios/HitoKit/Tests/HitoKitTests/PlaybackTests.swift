import Foundation
import Testing
@testable import HitoKit

@Suite struct PlaybackTests {
    let address = try! ServerAddressInput(parsing: "http://192.168.1.20:8080").candidates[0]
    let sources = #"[{"id":"s0","recording_id":"r1","provider":"youtube","label":"YouTube","ready":false,"can_prepare":true,"match_kind":"best_guess"},{"id":"s1","recording_id":"r1","provider":"local","label":"Local file","ready":true,"can_prepare":false,"match_kind":"verified"},{"id":"s2","recording_id":"r1","provider":"youtube","label":"YouTube","provider_label":"YouTube","ready":true,"can_prepare":false,"match_kind":"best_guess"}]"#
    let lease = #"{"id":"6f0b7c1e-0000-4000-8000-000000000001","expires_at":1900000000}"#

    func playback(_ fake: FakeServer) -> Playback {
        Playback(api: HitoAPI(address: address, token: "session-token-1", session: fake.session))
    }

    @Test func theFirstReadySourceInTheListenersOrderIsLeased() async throws {
        let fake = FakeServer(routes: [
            "GET /api/v1/recordings/r1/playback-sources": .json(200, sources),
            "POST /api/v1/playback/leases": .json(200, lease),
        ])
        let audio = try await playback(fake).prepare("r1")
        #expect(audio.sourceId == "s1")
        #expect(audio.sourceLabel == "Local file")
        #expect(audio.url.absoluteString == "http://192.168.1.20:8080/api/v1/playback/leases/6f0b7c1e-0000-4000-8000-000000000001/audio")
        #expect(audio.headers == ["Authorization": "Bearer session-token-1"])
        let begin = try #require(fake.requests.last)
        #expect(begin.header("x-hito-request") == "1")
        #expect(begin.json == ["recording_id": "r1", "source_id": "s1"])
    }

    @Test func aLeaseIsReleasedWhenTheTrackIsDone() async throws {
        let fake = FakeServer(routes: [
            "GET /api/v1/recordings/r1/playback-sources": .json(200, sources),
            "POST /api/v1/playback/leases": .json(200, lease),
            "DELETE /api/v1/playback/leases/6f0b7c1e-0000-4000-8000-000000000001": .text(204, ""),
        ])
        let audio = try await playback(fake).prepare("r1")
        #expect(audio.leaseId == "6f0b7c1e-0000-4000-8000-000000000001")
        await playback(fake).release(audio.leaseId)
        let release = try #require(fake.requests.last)
        #expect(release.line == "DELETE /api/v1/playback/leases/6f0b7c1e-0000-4000-8000-000000000001")
        #expect(release.header("x-hito-request") == "1")
    }

    @Test func afterASourceFailsTheNextReadyOneIsTried() async throws {
        let fake = FakeServer(routes: [
            "GET /api/v1/recordings/r1/playback-sources": .json(200, sources),
            "POST /api/v1/playback/leases": .json(200, lease),
        ])
        let audio = try await playback(fake).prepare("r1", skipping: ["s1"])
        #expect(audio.sourceId == "s2")
        #expect(audio.sourceLabel == "YouTube")
    }

    @Test func aTrackWithNoReadySourceSaysSo() async {
        let fake = FakeServer(routes: [
            "GET /api/v1/recordings/r1/playback-sources": .json(200, sources),
        ])
        await #expect(throws: Playback.Problem.noReadySource) {
            try await playback(fake).prepare("r1", skipping: ["s1", "s2"])
        }
        #expect(!fake.requests.contains { $0.line == "POST /api/v1/playback/leases" })
    }

    @Test func aRefusedLeaseMovesOnToTheNextSource() async throws {
        let fake = FakeServer(routes: [
            "GET /api/v1/recordings/r1/playback-sources": .json(200, sources),
            "POST /api/v1/playback/leases": .json(409, #"{"error":"Source is not available"}"#),
        ])
        await #expect(throws: Playback.Problem.noReadySource) {
            try await playback(fake).prepare("r1")
        }
        #expect(fake.requests.filter { $0.line == "POST /api/v1/playback/leases" }.compactMap { $0.json?["source_id"] } == ["s1", "s2"])
    }

    @Test func theSourcesForATrackAreListedInTheListenersOrder() async throws {
        let fake = FakeServer(routes: ["GET /api/v1/recordings/r1/playback-sources": .json(200, sources)])
        let listed = try await playback(fake).sources("r1")
        #expect(listed == [
            PlaybackSource(id: "s0", label: "YouTube", provider: "youtube", ready: false, bestGuess: true),
            PlaybackSource(id: "s1", label: "Local file", provider: "local", ready: true, bestGuess: false),
            PlaybackSource(id: "s2", label: "YouTube, second match", provider: "youtube", ready: true, bestGuess: true),
        ])
    }

    @Test func aLocalSourceThatIsNotReadyHasNoFile() async throws {
        let missing = #"[{"id":"s1","recording_id":"r1","provider":"local","label":"Local file","ready":false,"can_prepare":false,"match_kind":"verified"}]"#
        let fake = FakeServer(routes: ["GET /api/v1/recordings/r1/playback-sources": .json(200, missing)])
        let listed = try await playback(fake).sources("r1")
        #expect(listed.map(\.missingFile) == [true])
    }

    @Test func aThirdMatchFromTheSameProviderIsNamedSo() async throws {
        let three = #"[{"id":"a","recording_id":"r1","provider":"youtube","label":"YouTube","ready":true,"can_prepare":false,"match_kind":"verified"},{"id":"b","recording_id":"r1","provider":"youtube","label":"YouTube","ready":true,"can_prepare":false,"match_kind":"verified"},{"id":"c","recording_id":"r1","provider":"youtube","label":"YouTube","ready":false,"can_prepare":false,"match_kind":"verified"}]"#
        let fake = FakeServer(routes: ["GET /api/v1/recordings/r1/playback-sources": .json(200, three)])
        let listed = try await playback(fake).sources("r1")
        #expect(listed.map(\.label) == ["YouTube", "YouTube, second match", "YouTube, third match"])
    }

    @Test func aChosenSourceIsLeasedEvenWhenAnotherComesFirst() async throws {
        let fake = FakeServer(routes: [
            "GET /api/v1/recordings/r1/playback-sources": .json(200, sources),
            "POST /api/v1/playback/leases": .json(200, lease),
        ])
        let audio = try await playback(fake).prepare("r1", source: "s2")
        #expect(audio.sourceId == "s2")
        #expect(fake.requests.last?.json == ["recording_id": "r1", "source_id": "s2"])
    }
}
