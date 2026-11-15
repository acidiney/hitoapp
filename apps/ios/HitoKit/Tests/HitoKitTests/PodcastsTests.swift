import Foundation
import Testing
@testable import HitoKit

@Suite struct PodcastsTests {
    let address = try! ServerAddressInput(parsing: "http://192.168.1.20:8080").candidates[0]

    func podcasts(_ fake: FakeServer) -> Podcasts {
        Podcasts(api: HitoAPI(address: address, token: "session-token-1", session: fake.session))
    }

    func recording(_ id: String, duration: Int = 3_600_000) -> String {
        #"{"id":"\#(id)","title":"Episode \#(id)","inherited_title":"Episode \#(id)","inherited_artist":"Show","artist":"Show","album":null,"release_id":null,"duration_ms":\#(duration),"available":true,"plays_from_services":false,"liked":false}"#
    }

    @Test func subscriptionsShowTheirFeedState() async throws {
        let fake = FakeServer(routes: ["GET /api/v1/podcasts": .json(200, #"[{"subscription_id":"s1","title":"Fixture Show","refresh_pending":false,"refresh_failed":false,"content_revision":3,"last_refreshed_at":1700000000},{"subscription_id":"s2","title":null,"refresh_pending":true,"refresh_failed":false,"content_revision":0,"last_refreshed_at":null},{"subscription_id":"s3","title":"Broken","refresh_pending":false,"refresh_failed":true,"content_revision":1,"last_refreshed_at":1700000000}]"#)])
        let found = try await podcasts(fake).subscriptions()
        #expect(found.map(\.name) == ["Fixture Show", "Untitled podcast", "Broken"])
        #expect(found.map(\.status) == [nil, "Checking feed…", "Last feed check failed"])
    }

    @Test func subscribingSendsTheFeedAndChecksTheReceipt() async throws {
        let fake = FakeServer(routes: ["POST /api/v1/podcasts": .json(202, #"{"request_id":"r1","subscription_id":"s9","job_id":"j1"}"#)])
        let id = try await podcasts(fake).subscribe(feed: " https://feeds.example.test/show.xml ", request: "r1")
        #expect(id == "s9")
        #expect(fake.requests.first?.object?["feed_url"] as? String == "https://feeds.example.test/show.xml")
        let other = FakeServer(routes: ["POST /api/v1/podcasts": .json(202, #"{"request_id":"other","subscription_id":"s9","job_id":"j1"}"#)])
        await #expect(throws: Podcasts.Problem.mismatched) { try await podcasts(other).subscribe(feed: "https://feeds.example.test/show.xml", request: "r1") }
    }

    @Test func aRejectedFeedSaysWhy() async throws {
        let fake = FakeServer(routes: ["POST /api/v1/podcasts": .json(422, #"{"error":"invalid"}"#)])
        await #expect(throws: Podcasts.Problem.rejected) { try await podcasts(fake).subscribe(feed: "nope", request: "r1") }
    }

    @Test func episodesPageWithTheirSavedPositions() async throws {
        let fake = FakeServer(routes: [
            "GET /api/v1/podcasts/s1/episodes": .json(200, #"{"items":[{"episode_id":"e1","recording":\#(recording("r1"))},{"episode_id":"e2","recording":\#(recording("r2"))},{"episode_id":"e3","recording":\#(recording("r3"))}],"total":40,"limit":3,"next_cursor":"c2","content_revision":3}"#),
            "POST /api/v1/recordings/progress": .json(200, #"[{"recording_id":"r2","position_ms":1800000,"speed":1.0,"updated_at":1},{"recording_id":"r3","position_ms":3599000,"speed":1.0,"updated_at":1}]"#),
        ])
        let page = try await podcasts(fake).episodes("s1", after: nil)
        #expect(page.episodes.map(\.track.id) == ["r1", "r2", "r3"])
        #expect(page.episodes.map(\.state) == [.new("New · 1 h 0 min"), .remaining("30 min left", 0.5), .played])
        #expect(page.next == "c2")
        #expect(page.total == 40)
        #expect(page.lead?.track.id == "r2")
        #expect(page.leadLabel == "Continue · 30 min left")
        #expect(fake.requests.first?.query == "limit=50")
    }

    @Test func withNothingInProgressTheNewestEpisodeLeads() {
        let tracks = ["r1", "r2"].map { Track(id: $0, title: $0, artist: "Show", album: nil, releaseId: nil, durationMs: 600_000, available: true, playsFromServices: false, liked: false) }
        let page = Podcasts.EpisodePage(episodes: tracks.map { Podcasts.Episode(track: $0, positionMs: 0) }, next: nil, total: 2)
        #expect(page.lead?.track.id == "r1")
        #expect(page.leadLabel == "Play latest")
        #expect(Podcasts.Episode(track: tracks[0], positionMs: 0).state == .new("New · 10 min"))
    }

    @Test func refreshAndUnsubscribeSendTheirRequestIds() async throws {
        let fake = FakeServer(routes: [
            "POST /api/v1/podcasts/s1/refresh": .json(202, #"{"request_id":"r1","subscription_id":"s1","job_id":"j1"}"#),
            "POST /api/v1/podcasts/s1/unsubscribe": .json(200, #"{"request_id":"r2","subscription_id":"s1"}"#),
        ])
        try await podcasts(fake).refresh("s1", request: "r1")
        try await podcasts(fake).unsubscribe("s1", request: "r2")
        #expect(fake.requests.map { $0.object?["request_id"] as? String } == ["r1", "r2"])
    }
}
