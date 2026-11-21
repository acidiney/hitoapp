import Foundation
import Testing
@testable import HitoKit

@Suite struct PluginSignInTests {
    let address = try! ServerAddressInput(parsing: "http://192.168.1.20:8080").candidates[0]
    let begun = #"{"id":"a1","authorization_url":"https://provider.invalid/authorize?state=x","expires_at":1700000600}"#

    func signIn(_ fake: FakeServer) -> PluginSignIn {
        PluginSignIn(api: HitoAPI(address: address, token: "session-token-1", session: fake.session))
    }

    func attempt(returnSecret: String? = "return-secret-1") -> PluginSignIn.Attempt {
        PluginSignIn.Attempt(
            id: "a1",
            connection: "i1",
            expires: Date(timeIntervalSince1970: 1700000600),
            verifier: PluginSignIn.Verifier(value: "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            returnSecret: returnSecret
        )
    }

    @Test func eachAttemptGetsItsOwnLongURLSafeVerifier() {
        let first = PluginSignIn.Verifier()
        let second = PluginSignIn.Verifier()
        #expect(first != second)
        #expect(first.value.count >= 43)
        let allowed = CharacterSet(charactersIn: "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_")
        #expect(first.value.unicodeScalars.allSatisfy { allowed.contains($0) })
    }

    @Test func theVerifierHashIsUnpaddedBase64URLSHA256() {
        #expect(PluginSignIn.Verifier(value: "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk").hash == "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM")
    }

    @Test func beginningSendsTheRevisionAndOnlyTheVerifierHash() async throws {
        let fake = FakeServer(routes: ["POST /api/v1/plugins/i1/authorization": .json(201, begun)])
        let (attempt, url) = try await signIn(fake).begin("i1", revision: 3)
        #expect(attempt.id == "a1")
        #expect(attempt.connection == "i1")
        #expect(url == URL(string: "https://provider.invalid/authorize?state=x"))
        #expect(attempt.expires == Date(timeIntervalSince1970: 1700000600))
        #expect(attempt.returnSecret == nil)
        let body = try #require(fake.requests.first?.object)
        #expect(Set(body.keys) == ["revision", "verifier_hash"])
        #expect(body["revision"] as? Int == 3)
        #expect(body["verifier_hash"] as? String == attempt.verifier.hash)
        let sent = String(decoding: try #require(fake.requests.first?.body), as: UTF8.self)
        #expect(!sent.contains(attempt.verifier.value))
    }

    @Test func aServerThatCannotStartSignInSaysWhy() async throws {
        let fake = FakeServer(routes: ["POST /api/v1/plugins/i1/authorization": .json(422, #"{"error":"Configure the public origin before connecting this provider"}"#)])
        await #expect(throws: PluginSignIn.Refused(reason: "Configure the public origin before connecting this provider")) { try await signIn(fake).begin("i1", revision: 3) }
    }

    @Test func aServerWithoutAppSignInSendsTheListenerToTheWeb() async throws {
        let fake = FakeServer(routes: ["POST /api/v1/plugins/i1/authorization": .json(422, #"{"error":"Request JSON does not match the expected fields"}"#)])
        await #expect(throws: PluginSignIn.OnlyOnTheWeb()) { try await signIn(fake).begin("i1", revision: 3) }
    }

    @Test func theWebFallbackOpensPluginSettingsOnTheSameServer() {
        #expect(PluginSignIn.webSettings(on: address) == URL(string: "http://192.168.1.20:8080/settings?section=plugins"))
    }

    @Test func theReturnIsAcceptedOnlyForTheAttemptThisAppStarted() {
        func secret(_ text: String) -> String? {
            PluginSignIn.returnSecret(from: URL(string: text)!, for: "a1")
        }
        #expect(PluginSignIn.callbackScheme == "hito")
        #expect(secret("hito://plugin-authorization/return?attempt=a1&return=r-1_x") == "r-1_x")
        #expect(secret("hito://plugin-authorization/return?return=r-1_x&attempt=a1") == "r-1_x")
        #expect(secret("hito://plugin-authorization/return?attempt=a2&return=r-1_x") == nil)
        #expect(secret("hito://plugin-authorization/return?attempt=a1") == nil)
        #expect(secret("hito://plugin-authorization/return?attempt=a1&return=") == nil)
        #expect(secret("hito://plugin-authorization/return?attempt=a1&attempt=a2&return=r-1_x") == nil)
        #expect(secret("hito://plugin-authorization/return?attempt=a1&return=r-1_x&return=r-2") == nil)
        #expect(secret("hito://elsewhere/return?attempt=a1&return=r-1_x") == nil)
        #expect(secret("hito://plugin-authorization/other?attempt=a1&return=r-1_x") == nil)
        #expect(secret("hito://plugin-authorization/return/extra?attempt=a1&return=r-1_x") == nil)
        #expect(secret("https://plugin-authorization/return?attempt=a1&return=r-1_x") == nil)
        #expect(secret("hito://user@plugin-authorization/return?attempt=a1&return=r-1_x") == nil)
    }

    @Test func completionSendsBothSecretsAndReadsTheOutcome() async throws {
        let fake = FakeServer(routes: ["POST /api/v1/plugins/i1/authorization/a1/completion": .json(200, #"{"outcome":"connected"}"#)])
        let outcome = try await signIn(fake).complete(attempt(), returnSecret: "return-secret-1")
        #expect(outcome == .connected)
        let request = try #require(fake.requests.first)
        #expect(request.authorization == "Bearer session-token-1")
        #expect(request.json == ["verifier": "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk", "return_secret": "return-secret-1"])
    }

    @Test func everyOutcomeIsReadAndExplained() async throws {
        let names = ["connected", "denied", "provider_error", "exchange_failed", "expired", "changed", "used", "invalid", "something_new"]
        var outcomes: [PluginSignIn.Outcome] = []
        for name in names {
            let fake = FakeServer(routes: ["POST /api/v1/plugins/i1/authorization/a1/completion": .json(200, #"{"outcome":"\#(name)"}"#)])
            outcomes.append(try await signIn(fake).complete(attempt(), returnSecret: "return-secret-1"))
        }
        #expect(outcomes == [.connected, .denied, .providerError, .exchangeFailed, .expired, .changed, .used, .invalid, .invalid])
        #expect(outcomes.map { $0.problem("Spotify") } == [
            nil,
            "Spotify did not grant access. Nothing was connected.",
            "Spotify could not complete the sign-in. Try again.",
            "Spotify did not accept the approval. Try again.",
            "This sign-in expired. Try again.",
            "Spotify or your Hito sign-in changed while you were approving. Try again.",
            "This sign-in was already used. Try again if you still need to connect.",
            "This sign-in could not be confirmed. Try again.",
            "This sign-in could not be confirmed. Try again.",
        ])
    }

    @Test func aCompletionForAnotherSessionIsGone() async throws {
        let fake = FakeServer(routes: ["POST /api/v1/plugins/i1/authorization/a1/completion": .json(404, #"{"error":"Resource not found"}"#)])
        await #expect(throws: HitoAPI.Problem.refused(404)) { try await signIn(fake).complete(attempt(), returnSecret: "return-secret-1") }
    }

    @Test func anAttemptSurvivesBeingKeptForARestart() throws {
        let kept = try JSONEncoder().encode(attempt())
        #expect(try JSONDecoder().decode(PluginSignIn.Attempt.self, from: kept) == attempt())
    }

    @Test func afterARestartAReturnedAttemptIsFinishedWithTheKeptSecrets() async throws {
        let fake = FakeServer(routes: [
            "GET /api/v1/plugins/i1/authorization/a1": .json(200, #"{"id":"a1","status":"returned","expires_at":1700000600}"#),
            "POST /api/v1/plugins/i1/authorization/a1/completion": .json(200, #"{"outcome":"connected"}"#),
        ])
        #expect(try await signIn(fake).resume(attempt()) == .finished(.connected))
        #expect(fake.requests.map(\.line) == ["GET /api/v1/plugins/i1/authorization/a1", "POST /api/v1/plugins/i1/authorization/a1/completion"])
        #expect(fake.requests.last?.json == ["verifier": "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk", "return_secret": "return-secret-1"])
    }

    @Test func checkingAnAttemptSaysWhetherToFinishWaitOrStartAgain() async throws {
        func resumed(_ answer: FakeServer.Answer, secret: String? = "return-secret-1") async throws -> (PluginSignIn.Resumption, [String]) {
            let fake = FakeServer(routes: ["GET /api/v1/plugins/i1/authorization/a1": answer])
            let resumption = try await signIn(fake).resume(attempt(returnSecret: secret))
            return (resumption, fake.requests.map(\.line))
        }
        let status = { (state: String) in FakeServer.Answer.json(200, #"{"id":"a1","status":"\#(state)","expires_at":1700000600}"#) }
        #expect(try await resumed(status("pending")).0 == .notReturned)
        #expect(try await resumed(status("exchanging")).0 == .waiting)
        #expect(try await resumed(status("completed")).0 == .finished(.connected))
        #expect(try await resumed(status("failed")).0 == .ended)
        #expect(try await resumed(.json(404, #"{"error":"Resource not found"}"#)).0 == .ended)
        let unfinishable = try await resumed(status("returned"), secret: nil)
        #expect(unfinishable.0 == .ended)
        #expect(unfinishable.1 == ["GET /api/v1/plugins/i1/authorization/a1"])
    }

    @Test func checksBackOffLikeTheWeb() {
        #expect(PluginSignIn.delays.prefix(6).map { ($0 * 100).rounded() / 100 } == [1, 1.5, 2.25, 3.38, 5, 5])
    }
}
