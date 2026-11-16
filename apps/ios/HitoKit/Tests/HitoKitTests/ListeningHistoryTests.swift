import Foundation
import Testing
@testable import HitoKit

@Suite struct ListeningHistoryTests {
    let address = try! ServerAddressInput(parsing: "http://192.168.1.20:8080").candidates[0]

    func history(_ fake: FakeServer) -> ListeningHistory {
        ListeningHistory(api: HitoAPI(address: address, token: "session-token-1", session: fake.session))
    }

    func row(_ id: String, position: Int) -> String {
        #"{"id":"\#(id)","title":"Song \#(id)","inherited_title":"Song \#(id)","inherited_artist":"Band","artist":"Band","album":null,"release_id":null,"duration_ms":200000,"position_ms":\#(position),"speed":1.0,"updated_at":1700000000,"available":true,"plays_from_services":false,"liked":false}"#
    }

    @Test func historyListsRecentPlaysWithWhereTheyStopped() async throws {
        let fake = FakeServer(routes: [
            "GET /api/v1/activity": .json(200, #"{"history":[\#(row("r1", position: 61000)),\#(row("r2", position: 0))]}"#),
            "GET /api/v1/history/preferences": .json(200, #"{"enabled":true}"#),
        ])
        let found = try await history(fake).load()
        #expect(found.entries.map(\.track.id) == ["r1", "r2"])
        #expect(found.entries.map(\.positionMs) == [61000, 0])
        #expect(found.enabled)
        #expect(found.summary == "2 recently played tracks")
    }

    @Test func pausedHistorySaysSo() async throws {
        let fake = FakeServer(routes: [
            "GET /api/v1/activity": .json(200, #"{"history":[\#(row("r1", position: 0))]}"#),
            "GET /api/v1/history/preferences": .json(200, #"{"enabled":false}"#),
        ])
        let found = try await history(fake).load()
        #expect(!found.enabled)
        #expect(found.summary == "1 recently played track")
    }
}
