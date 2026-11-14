import Foundation
import Testing
@testable import HitoKit

@Suite struct SessionLockTests {
    let address = try! ServerAddressInput(parsing: "http://192.168.1.20:8080").candidates[0]

    @Test func readsWhetherThisSessionIsLocked() async throws {
        let fake = FakeServer(routes: ["GET /api/v1/account/lock": .json(200, #"{"pin_enabled":true,"idle_minutes":5,"locked":true}"#)])
        let status = try await AccountSession(session: fake.session).lockStatus(at: address, token: "t1")
        #expect(status.locked)
        #expect(fake.requests.first?.authorization == "Bearer t1")
    }

    @Test func theRightPinUnlocks() async throws {
        let fake = FakeServer(routes: ["POST /api/v1/account/unlock": .json(200, #"{"pin_enabled":true,"idle_minutes":5,"locked":false}"#)])
        let result = try await AccountSession(session: fake.session).unlock(at: address, token: "t1", pin: "1234")
        #expect(result == .unlocked)
        let request = try #require(fake.requests.first)
        #expect(request.json == ["pin": "1234"])
        #expect(request.header("x-hito-request") == "1")
    }

    @Test func aWrongPinKeepsTheSessionLocked() async throws {
        let fake = FakeServer(routes: ["POST /api/v1/account/unlock": .json(200, #"{"pin_enabled":true,"idle_minutes":5,"locked":true}"#)])
        let result = try await AccountSession(session: fake.session).unlock(at: address, token: "t1", pin: "0000")
        #expect(result == .wrongPin)
    }

    @Test func tooManyWrongPinsEndTheSession() async throws {
        let fake = FakeServer(routes: ["POST /api/v1/account/unlock": .json(401, #"{"error":"Sign in again"}"#)])
        let result = try await AccountSession(session: fake.session).unlock(at: address, token: "t1", pin: "0000")
        #expect(result == .signedOut)
    }

    @Test func aLockedSessionIsNotReportedAsUnreachable() async {
        let fake = FakeServer(routes: ["GET /api/v1/me": .json(423, #"{"error":"Locked"}"#)])
        await #expect(throws: AccountSession.Problem.locked) {
            try await AccountSession(session: fake.session).currentUser(at: address, token: "t1")
        }
    }
}
