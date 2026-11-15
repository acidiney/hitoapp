import Foundation
import Testing
@testable import HitoKit

@Suite struct ReviewQueueTests {
    let address = try! ServerAddressInput(parsing: "http://192.168.1.20:8080").candidates[0]
    let base = "/api/v1/artist-matching/review-queue"

    func queue(_ fake: FakeServer) -> ReviewQueue {
        ReviewQueue(api: HitoAPI(address: address, token: "session-token-1", session: fake.session))
    }

    var page: String {
        #"{"items":[{"artist_id":"c1","name":"Fixture Band","undecided_tracks":3,"needs_search":false,"search_recording_id":null,"candidates":[{"provider_artist_id":"m1","name":"Fixture Band","disambiguation":"UK rock band","kind":"Group","country":"GB","corroborated_titles":2,"in_library":false,"origin":"name_search"},{"provider_artist_id":"m2","name":"Fixture Band","disambiguation":null,"kind":null,"country":null,"corroborated_titles":1,"in_library":true,"origin":"matched_on_other_tracks"}]},{"artist_id":"c2","name":"Unknown Duo","undecided_tracks":1,"needs_search":true,"search_recording_id":"r7","candidates":[]}],"total":51,"offset":0,"limit":50}"#
    }

    @Test func theQueueListsCreditsWithTheirCandidates() async throws {
        let fake = FakeServer(routes: ["GET \(base)": .json(200, page)])
        let read = try await queue(fake).waiting(offset: 0)
        #expect(read.total == 51)
        #expect(read.items.map(\.name) == ["Fixture Band", "Unknown Duo"])
        #expect(read.items[0].tracks == "3 tracks")
        #expect(read.items[1].needsSearch)
        #expect(read.items[1].searchRecordingId == "r7")
        #expect(fake.requests.last?.query == "limit=50&offset=0")
    }

    @Test func candidatesDescribeThemselvesLikeTheWeb() async throws {
        let fake = FakeServer(routes: ["GET \(base)": .json(200, page)])
        let candidates = try await queue(fake).waiting(offset: 0).items[0].candidates
        #expect(candidates[0].details == "UK rock band · Group · United Kingdom · 2 titles match your library")
        #expect(candidates[0].label == "Fixture Band, UK rock band")
        #expect(candidates[1].details == "Matched on 1 other track of this credit · already in your artists")
    }

    @Test func dismissedSuggestionsArePagedSeparately() async throws {
        let fake = FakeServer(routes: ["GET \(base)/dismissed": .json(200, page)])
        _ = try await queue(fake).dismissed(offset: 50)
        #expect(fake.requests.last?.query == "limit=50&offset=50")
    }

    @Test func confirmingFollowsTheServerUntilEveryTrackIsApplied() async throws {
        let fake = FakeServer(routes: ["POST \(base)/c1/confirm": .sequence([
            .json(200, #"{"request_id":"00000000-0000-0000-0000-000000000001","artist_id":"c1","provider_artist_id":"m1","applied":500,"kept":0,"remaining":10,"next_after":"cursor-1"}"#),
            .json(200, #"{"request_id":"00000000-0000-0000-0000-000000000002","artist_id":"c1","provider_artist_id":"m1","applied":10,"kept":0,"remaining":0,"next_after":null}"#),
        ])])
        try await queue(fake).confirm(credit: "c1", artist: "m1")
        let bodies = fake.requests.compactMap(\.object)
        #expect(bodies.count == 2)
        #expect(bodies.map { $0["provider_artist_id"] as? String } == ["m1", "m1"])
        #expect(bodies[0]["after"] == nil || bodies[0]["after"] is NSNull)
        #expect(bodies[1]["after"] as? String == "cursor-1")
        #expect(bodies[0]["request_id"] as? String != bodies[1]["request_id"] as? String)
    }

    @Test func dismissingAndRestoringNameTheCandidateOrTheWholeCredit() async throws {
        let fake = FakeServer(routes: [
            "POST \(base)/c1/dismiss": .acknowledging,
            "POST \(base)/c1/restore": .acknowledging,
        ])
        try await queue(fake).dismiss(credit: "c1", artist: nil)
        #expect(fake.requests.last?.object?.isEmpty == true)
        try await queue(fake).restore(credit: "c1", artist: "m2")
        #expect(fake.requests.last?.object?["provider_artist_id"] as? String == "m2")
    }
}
