import Foundation
import Testing
@testable import HitoKit

@Suite struct PlaylistEditTests {
    let address = try! ServerAddressInput(parsing: "http://192.168.1.20:8080").candidates[0]

    func playlists(_ fake: FakeServer) -> Playlists {
        Playlists(api: HitoAPI(address: address, token: "session-token-1", session: fake.session))
    }

    func slots(_ positions: [Int]) -> [PlaylistDraft.Slot] {
        positions.enumerated().map { PlaylistDraft.Slot(id: "e\($0.offset)", position: $0.element) }
    }

    func moves(_ pairs: [(String, Int)]) -> [PlaylistDraft.Change] {
        pairs.map { .move($0.0, to: $0.1) }
    }

    @Test func movingDownSwapsPositionWithTheNeighbour() {
        var draft = PlaylistDraft(name: "Mix", entries: slots([0, 1, 2]))
        let moved1 = draft.move("e0", by: 1)
        #expect(moved1)
        #expect(draft.entries == ["e1", "e0", "e2"])
        #expect(draft.changes == moves([("e1", 0), ("e0", 1)]))
    }

    @Test func movingUpSwapsWithTheOneAboveKeepingSparsePositions() {
        var draft = PlaylistDraft(name: "Mix", entries: slots([10, 20, 30]))
        let moved2 = draft.move("e2", by: -1)
        #expect(moved2)
        #expect(draft.changes == moves([("e2", 20), ("e1", 30)]))
    }

    @Test func movingPastTheEndsChangesNothing() {
        var draft = PlaylistDraft(name: "Mix", entries: slots([0, 1]))
        let moved3 = draft.move("e0", by: -1)
        #expect(moved3)
        let moved4 = draft.move("e1", by: 1)
        #expect(moved4)
        #expect(draft.entries == ["e0", "e1"])
        #expect(draft.changes.isEmpty)
    }

    @Test func tiedPositionsAreRenumberedOnlyAroundTheMove() {
        var draft = PlaylistDraft(name: "Mix", entries: slots([5, 5, 5]))
        let moved5 = draft.move("e0", by: 1)
        #expect(moved5)
        #expect(draft.entries == ["e1", "e0", "e2"])
        #expect(draft.changes == moves([("e0", 6), ("e2", 7)]))
    }

    @Test func oneMoveInALargePlaylistChangesTwoPositions() {
        var draft = PlaylistDraft(name: "Mix", entries: slots(Array(0..<5000)))
        let moved6 = draft.move("e2500", by: 1)
        #expect(moved6)
        #expect(draft.changes == moves([("e2501", 2500), ("e2500", 2501)]))
    }

    @Test func aTieThatWouldShiftThousandsIsRefused() {
        var draft = PlaylistDraft(name: "Mix", entries: slots((0..<5000).map { $0 <= 2500 ? $0 : $0 - 1 }))
        let moved7 = draft.move("e2500", by: 1)
        #expect(!moved7)
        #expect(draft.entries.first == "e0")
        #expect(draft.changes.isEmpty)
    }

    @Test func aSequenceOfMovesSendsOnlyTheNetPositions() {
        var draft = PlaylistDraft(name: "Mix", entries: slots([0, 1, 2]))
        draft.move("e0", by: 1)
        draft.move("e0", by: 1)
        #expect(draft.entries == ["e1", "e2", "e0"])
        #expect(draft.changes == moves([("e1", 0), ("e2", 1), ("e0", 2)]))
    }

    @Test func movingBackLeavesNothingToSave() {
        var draft = PlaylistDraft(name: "Mix", entries: slots([0, 1, 2]))
        draft.move("e1", by: -1)
        draft.move("e1", by: 1)
        #expect(draft.entries == ["e0", "e1", "e2"])
        #expect(draft.changes.isEmpty)
    }

    @Test func removedEntriesLeaveTheOrderAndAreSentFirst() {
        var draft = PlaylistDraft(name: "Mix", entries: slots([0, 1, 2, 3]))
        draft.remove("e1")
        draft.move("e3", by: -1)
        #expect(draft.entries == ["e0", "e3", "e2"])
        #expect(draft.changes == [.remove("e1")] + moves([("e3", 2), ("e2", 3)]))
    }

    @Test func aRenameIsSentOnlyWhenTheNameChanged() {
        var draft = PlaylistDraft(name: "Mix", entries: slots([0]))
        #expect(draft.changes.isEmpty)
        draft.name = "  Road trip "
        #expect(draft.changes == [.rename("Road trip")])
        draft.name = "   "
        #expect(draft.changes.isEmpty)
    }

    @Test func savingSendsEveryChangeInOneSyncBatch() async throws {
        let fake = FakeServer(routes: ["POST /api/v1/sync": .acknowledging])
        try await playlists(fake).apply([.rename("Road trip"), .remove("e2"), .move("e4", to: 1)], to: "p1", device: "d1")
        let operations = try #require(fake.requests.first?.object?["operations"] as? [[String: Any]])
        let actions = operations.compactMap { $0["action"] as? [String: Any] }
        #expect(actions.map { $0["kind"] as? String } == ["playlist_rename", "entry_remove", "entry_move"])
        #expect(actions[0]["name"] as? String == "Road trip")
        #expect(actions[2]["position"] as? Int == 1)
        #expect(actions.allSatisfy { $0["playlist_id"] as? String == "p1" })
    }

    @Test func aNewPlaylistGetsItsOwnId() async throws {
        let fake = FakeServer(routes: ["POST /api/v1/sync": .acknowledging])
        let id = try await playlists(fake).create(named: "Fresh", device: "d1")
        let action = try #require(((fake.requests.first?.object?["operations"] as? [[String: Any]])?.first?["action"]) as? [String: Any])
        #expect(action["kind"] as? String == "playlist_create")
        #expect(action["playlist_id"] as? String == id)
        #expect(action["name"] as? String == "Fresh")
    }

    @Test func deletingAPlaylistIsOneChange() async throws {
        let fake = FakeServer(routes: ["POST /api/v1/sync": .acknowledging])
        try await playlists(fake).delete("p1", device: "d1")
        let action = try #require(((fake.requests.first?.object?["operations"] as? [[String: Any]])?.first?["action"]) as? [String: Any])
        #expect(action["kind"] as? String == "playlist_delete")
    }
}
