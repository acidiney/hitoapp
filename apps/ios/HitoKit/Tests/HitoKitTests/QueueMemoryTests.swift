import Foundation
import Testing
@testable import HitoKit

@Suite struct QueueMemoryTests {
    let address = try! ServerAddressInput(parsing: "http://192.168.1.20:8080").candidates[0]

    func track(_ id: String, ms: Int = 200_000) -> Track {
        Track(id: id, title: "Song \(id)", artist: "Band", album: nil, releaseId: nil, durationMs: ms, available: true, playsFromServices: false, liked: false)
    }

    func memory() -> QueueMemory {
        QueueMemory(defaults: UserDefaults(suiteName: "queue-memory-\(UUID().uuidString)")!, account: "account-1")
    }

    @Test func aSavedQueueComesBackWithItsTrackAndPosition() {
        let memory = memory()
        memory.save(ids: ["a", "b", "c"], index: 1, position: 42.5, context: "Fixture Album")
        #expect(memory.load() == StoredQueue(ids: ["a", "b", "c"], index: 1, position: 42.5, context: "Fixture Album"))
        memory.forget()
        #expect(memory.load() == nil)
    }

    @Test func eachAccountKeepsItsOwnQueue() {
        let defaults = UserDefaults(suiteName: "queue-memory-\(UUID().uuidString)")!
        QueueMemory(defaults: defaults, account: "account-1").save(ids: ["a"], index: 0, position: 1, context: "Mine")
        #expect(QueueMemory(defaults: defaults, account: "account-2").load() == nil)
    }

    @Test func aLongQueueKeepsFiftyBehindAndFiveHundredInAll() {
        let ids = (0..<900).map { "r\($0)" }
        let memory = memory()
        memory.save(ids: ids, index: 300, position: 0, context: "Long")
        let stored = memory.load()
        #expect(stored?.ids.first == "r250")
        #expect(stored?.ids.count == 500)
        #expect(stored?.ids[stored!.index] == "r300")
    }

    @Test func tracksThatAreGoneAreLeftOutAndThePlaceIsKept() {
        let stored = StoredQueue(ids: ["a", "b", "c", "d"], index: 2, position: 30, context: "Mix")
        let kept = stored.restore([track("a"), track("c"), track("d")])
        #expect(kept?.tracks.map(\.id) == ["a", "c", "d"])
        #expect(kept?.index == 1)
        #expect(kept?.position == 30)
        let gone = stored.restore([track("a"), track("b"), track("d")])
        #expect(gone?.tracks[gone!.index].id == "d")
        #expect(gone?.position == 0)
        #expect(stored.restore([]) == nil)
        let short = StoredQueue(ids: ["a"], index: 0, position: 500, context: "Mix").restore([track("a", ms: 90_000)])
        #expect(short?.position == 90)
    }

    @Test func recordingsAreLookedUpTogether() async throws {
        let recording = #"{"id":"r1","library_id":"l1","title":"Song","inherited_title":"Song","inherited_artist":"Band","artist":"Band","duration_ms":200000,"available":true,"plays_from_services":false,"liked":false,"album":null,"release_id":null}"#
        let fake = FakeServer(routes: ["POST /api/v1/recordings/lookup": .json(200, "[\(recording)]")])
        let library = Library(api: HitoAPI(address: address, token: "session-token-1", session: fake.session))
        #expect(try await library.lookup(["r1", "r2"]).map(\.id) == ["r1"])
        #expect(fake.requests.first?.object?["recording_ids"] as? [String] == ["r1", "r2"])
    }
}
