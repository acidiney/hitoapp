import Foundation
import Testing
@testable import HitoKit

@Suite struct ListeningProgressTests {
    let address = try! ServerAddressInput(parsing: "http://192.168.1.20:8080").candidates[0]

    @Test func progressIsSavedAsASyncChangeFromThisDevice() async throws {
        let fake = FakeServer(routes: ["POST /api/v1/sync": .acknowledging])
        let progress = ListeningProgress(api: HitoAPI(address: address, token: "session-token-1", session: fake.session), device: "device-1")
        try await progress.save("r1", position: 42.75, speed: 1)
        let body = try #require(fake.requests.first?.object)
        #expect(body["device_id"] as? String == "device-1")
        let action = try #require((body["operations"] as? [[String: Any]])?.first?["action"] as? [String: Any])
        #expect(action["kind"] as? String == "progress_set")
        #expect(action["recording_id"] as? String == "r1")
        #expect(action["position_ms"] as? Int == 42750)
        #expect(action["speed"] as? Double == 1)
    }

    @Test func anAudiobookChapterIsSavedWithItsProgress() async throws {
        let fake = FakeServer(routes: ["POST /api/v1/sync": .acknowledging])
        let progress = ListeningProgress(api: HitoAPI(address: try ServerAddressInput(parsing: "http://192.168.1.20:8080").candidates[0], token: "session-token-1", session: fake.session), device: "d1")
        try await progress.save("r2", position: 12.5, speed: 1, chapter: AudiobookAnchor(audiobookId: "b1", entryId: "e2", recordingId: "r2"))
        let actions = try #require((fake.requests.first?.object?["operations"] as? [[String: Any]])?.compactMap { $0["action"] as? [String: Any] })
        #expect(actions.map { $0["kind"] as? String } == ["progress_set", "audiobook_chapter_set"])
        let chapter = try #require(actions[1]["chapter"] as? [String: Any])
        #expect(chapter["audiobook_id"] as? String == "b1")
        #expect(chapter["entry_id"] as? String == "e2")
        #expect(chapter["recording_id"] as? String == "r2")
    }

    @Test func aCheckpointIsTakenEveryTenSecondsOfListening() {
        var checkpoint = ProgressCheckpoint()
        #expect(checkpoint.observe(0) == nil)
        #expect(checkpoint.observe(9.5) == nil)
        #expect(checkpoint.observe(10.2) == 10.2)
        #expect(checkpoint.observe(15) == nil)
        #expect(checkpoint.observe(20.3) == 20.3)
    }

    @Test func pausingSavesWhereYouStoppedUnlessItIsAlreadySaved() {
        var checkpoint = ProgressCheckpoint()
        #expect(checkpoint.settle(0) == nil)
        #expect(checkpoint.settle(4) == 4)
        #expect(checkpoint.settle(4) == nil)
        checkpoint = ProgressCheckpoint()
        #expect(checkpoint.observe(12) == 12)
        #expect(checkpoint.settle(12) == nil)
        #expect(checkpoint.settle(13) == 13)
    }
}
