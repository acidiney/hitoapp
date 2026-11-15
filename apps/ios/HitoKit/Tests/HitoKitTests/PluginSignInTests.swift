import Foundation
import Testing
@testable import HitoKit

@Suite struct PluginSignInTests {
    let address = try! ServerAddressInput(parsing: "http://192.168.1.20:8080").candidates[0]

    func signIn(_ fake: FakeServer) -> PluginSignIn {
        PluginSignIn(api: HitoAPI(address: address, token: "session-token-1", session: fake.session))
    }

    @Test func beginningSendsTheConnectionRevision() async throws {
        let fake = FakeServer(routes: ["POST /api/v1/plugins/i1/authorization": .json(201, #"{"id":"a1","authorization_url":"https://provider.invalid/authorize?state=x","expires_at":1700000600}"#)])
        let attempt = try await signIn(fake).begin("i1", revision: 3)
        #expect(attempt.id == "a1")
        #expect(attempt.url == URL(string: "https://provider.invalid/authorize?state=x"))
        #expect(attempt.expires == Date(timeIntervalSince1970: 1700000600))
        #expect(fake.requests.first?.object?["revision"] as? Int == 3)
    }

    @Test func aServerThatCannotStartSignInSaysWhy() async throws {
        let fake = FakeServer(routes: ["POST /api/v1/plugins/i1/authorization": .json(422, #"{"error":"Configure the public origin before connecting this provider"}"#)])
        await #expect(throws: PluginSignIn.Refused(reason: "Configure the public origin before connecting this provider")) { try await signIn(fake).begin("i1", revision: 3) }
    }

    @Test func statusFollowsTheAttemptUntilItEnds() async throws {
        let fake = FakeServer(routes: ["GET /api/v1/plugins/i1/authorization/a1": .sequence([
            .json(200, #"{"id":"a1","status":"pending","expires_at":1700000600}"#),
            .json(200, #"{"id":"a1","status":"exchanging","expires_at":1700000600}"#),
            .json(200, #"{"id":"a1","status":"completed","expires_at":1700000600}"#),
            .json(404, #"{"error":"Resource not found"}"#),
        ])])
        let api = signIn(fake)
        #expect(try await api.status("i1", attempt: "a1") == .waiting)
        #expect(try await api.status("i1", attempt: "a1") == .waiting)
        #expect(try await api.status("i1", attempt: "a1") == .completed)
        #expect(try await api.status("i1", attempt: "a1") == .gone)
    }

    @Test func checksBackOffLikeTheWeb() {
        #expect(PluginSignIn.delays.prefix(6).map { ($0 * 100).rounded() / 100 } == [1, 1.5, 2.25, 3.38, 5, 5])
    }
}
