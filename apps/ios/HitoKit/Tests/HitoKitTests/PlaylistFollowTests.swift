import Foundation
import Testing
@testable import HitoKit

@Suite struct PlaylistFollowTests {
    let address = try! ServerAddressInput(parsing: "http://192.168.1.20:8080").candidates[0]

    func following(_ fake: FakeServer) -> PlaylistFollowing {
        PlaylistFollowing(api: HitoAPI(address: address, token: "session-token-1", session: fake.session))
    }

    func status(_ state: String, interval: String = "daily", review: String = "null") -> String {
        #"{"playlist_id":"p1","generation":4,"content_revision":9,"interval":"\#(interval)","snapshot":"s","last_refreshed_at":1700000000,"next_refresh_at":1700086400,"status":"\#(state)","error":null,"review":\#(review)}"#
    }

    func review(_ changes: String, resumable: Bool = true) -> String {
        #"{"connection_name":"Mixes","installation_revision":3,"service_revision":7,"resumable":\#(resumable),"changes":[\#(changes)]}"#
    }

    @Test func statusReadsTheScheduleAndLabels() async throws {
        let fake = FakeServer(routes: ["GET /api/v1/playlists/p1/follow": .json(200, status("running", interval: "hourly"))])
        let follow = try await following(fake).status("p1")
        #expect(follow.state == .running)
        #expect(follow.state.label == "Updating")
        #expect(follow.busy)
        #expect(follow.intervalLabel == "Every hour")
        #expect(follow.lastChecked == Date(timeIntervalSince1970: 1700000000))
        #expect(follow.nextCheck == Date(timeIntervalSince1970: 1700086400))
        #expect(follow.contentRevision == 9)
    }

    @Test func refreshSendsOneRequestIdAndReportsABusyPlaylist() async throws {
        let fake = FakeServer(routes: ["POST /api/v1/playlists/p1/follow/refresh": .sequence([
            .json(202, #"{"request_id":"r1","job_id":"j1","playlist_id":"p1"}"#),
            .json(409, #"{"error":"busy"}"#),
        ])])
        try await following(fake).refresh("p1", request: "r1")
        #expect(fake.requests.first?.object?["request_id"] as? String == "r1")
        await #expect(throws: PlaylistFollowing.Problem.busy) { try await following(fake).refresh("p1", request: "r2") }
    }

    @Test func resumeSendsTheReviewedRevisionsInOneSyncChange() async throws {
        let fake = FakeServer(routes: [
            "GET /api/v1/playlists/p1/follow": .json(200, status("paused", review: review(#"{"kind":"credential","before":["a"],"after":["b"]}"#))),
            "POST /api/v1/sync": .acknowledging,
        ])
        let follow = try await following(fake).status("p1")
        try await following(fake).resume("p1", follow, device: "d1")
        let action = try #require(((fake.requests.last?.object?["operations"] as? [[String: Any]])?.first?["action"]) as? [String: Any])
        #expect(action["kind"] as? String == "playlist_follow_resume")
        #expect(action["playlist_id"] as? String == "p1")
        #expect(action["expected_generation"] as? Int == 4)
        #expect(action["installation_revision"] as? Int == 3)
        #expect(action["service_revision"] as? Int == 7)
    }

    @Test func reviewSentencesMatchTheWeb() throws {
        func sentence(_ changes: String) throws -> String {
            let data = Data(review(changes).utf8)
            return try JSONDecoder().decode(PlaylistFollowing.Review.self, from: data).sentence
        }
        #expect(try sentence(#"{"kind":"removed","before":[],"after":[]}"#) == "The Mixes connection was removed or replaced.")
        #expect(try sentence(#"{"kind":"credential","before":[],"after":[]}"#) == "The saved access token for Mixes changed.")
        #expect(try sentence(#"{"kind":"capabilities","before":["playlists"],"after":["search"]}"#) == "The Mixes connection no longer offers playlists.")
        #expect(try sentence(#"{"kind":"capabilities","before":["playlists"],"after":["playlists","artwork","lyrics","search"]}"#) == "The Mixes connection now also offers song search, lyrics and album covers.")
        #expect(try sentence(#"{"kind":"capabilities","before":["playlists","search"],"after":["playlists"]}"#) == "The Mixes connection changed.")
        #expect(try sentence(#"{"kind":"removed","before":[],"after":[]},{"kind":"credential","before":[],"after":[]}"#) == "The Mixes connection changed.")
    }

    @Test func aRemovedConnectionNamesTheServiceToConnectAgain() throws {
        let removed = #"{"connection_name":"Mixes","installation_revision":0,"service_revision":0,"resumable":false,"changes":[{"kind":"removed","before":["Connected"],"after":["Removed or replaced"]}],"reconnect_service_id":"s1"}"#
        let parsed = try JSONDecoder().decode(PlaylistFollowing.Review.self, from: Data(removed.utf8))
        #expect(parsed.removed)
        #expect(parsed.reconnectServiceId == "s1")
        #expect(parsed.reconnectTitle == "Connect Mixes again")
        #expect(parsed.reconnectNote == "Connect Mixes again and this playlist resumes updating.")
        let changed = try JSONDecoder().decode(PlaylistFollowing.Review.self, from: Data(review(#"{"kind":"credential","before":[],"after":[]}"#).utf8))
        #expect(!changed.removed)
        #expect(changed.reconnectServiceId == nil)
    }

    @Test func changeValuesReadAsPlainWords() throws {
        let change = try JSONDecoder().decode(PlaylistFollowing.Change.self, from: Data(#"{"kind":"capabilities","before":[],"after":["source_links","search"]}"#.utf8))
        #expect(change.label == "Abilities")
        #expect(PlaylistFollowing.Change.value(change.before) == "None")
        #expect(PlaylistFollowing.Change.value(change.after) == "search, source links")
    }
}
