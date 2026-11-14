import Foundation
import Testing
@testable import HitoKit

@Suite struct LikesTests {
    let address = try! ServerAddressInput(parsing: "http://192.168.1.20:8080").candidates[0]

    @Test func likingATrackSendsOneAcknowledgedChange() async throws {
        let fake = FakeServer(routes: ["POST /api/v1/sync": .acknowledging])
        try await Likes(api: HitoAPI(address: address, token: "session-token-1", session: fake.session), device: "device-1")
            .setLiked("r1", true)
        let request = try #require(fake.requests.first)
        #expect(request.header("x-hito-request") == "1")
        let body = try #require(request.object)
        #expect(body["device_id"] as? String == "device-1")
        let operation = try #require((body["operations"] as? [[String: Any]])?.first)
        let action = try #require(operation["action"] as? [String: Any])
        #expect(action["kind"] as? String == "like_set")
        #expect(action["recording_id"] as? String == "r1")
        #expect(action["liked"] as? Bool == true)
    }

    @Test func aChangeTheServerDidNotAcknowledgeFails() async {
        let fake = FakeServer(routes: ["POST /api/v1/sync": .json(200, #"{"acknowledged":[]}"#)])
        await #expect(throws: Likes.Problem.notAcknowledged) {
            try await Likes(api: HitoAPI(address: address, token: "session-token-1", session: fake.session), device: "device-1")
                .setLiked("r1", false)
        }
    }
}
