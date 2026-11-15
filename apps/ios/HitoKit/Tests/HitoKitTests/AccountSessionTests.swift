import Foundation
import Testing
@testable import HitoKit

@Suite struct AccountSessionTests {
    let address = try! ServerAddressInput(parsing: "http://192.168.1.20:8080").candidates[0]
    let session = #"{"token":"session-token-1","user":{"id":"u1","username":"admin","admin":true}}"#

    @Test func creatingTheAdministratorSignsIn() async throws {
        let fake = FakeServer(routes: ["POST /api/v1/setup": .json(201, session)])
        let signedIn = try await AccountSession(session: fake.session)
            .createAdministrator(at: address, username: "admin", password: "correct horse")
        #expect(signedIn.token == "session-token-1")
        #expect(signedIn.user.admin)
        let request = try #require(fake.requests.first)
        #expect(request.header("x-hito-request") == "1")
        #expect(request.header("content-type") == "application/json")
        #expect(request.json == ["username": "admin", "password": "correct horse"])
    }

    @Test func signingInReturnsTheSessionToken() async throws {
        let fake = FakeServer(routes: ["POST /api/v1/login": .json(200, session)])
        let signedIn = try await AccountSession(session: fake.session)
            .signIn(at: address, username: "admin", password: "correct horse")
        #expect(signedIn.token == "session-token-1")
        #expect(fake.requests.map(\.line) == ["POST /api/v1/login"])
    }

    @Test func aServerAlreadySetUpSendsThePersonToSignIn() async {
        let fake = FakeServer(routes: ["POST /api/v1/setup": .json(409, #"{"error":"Setup is already complete"}"#)])
        await #expect(throws: AccountSession.Problem.alreadySetUp) {
            try await AccountSession(session: fake.session)
                .createAdministrator(at: address, username: "admin", password: "correct horse")
        }
    }

    @Test func wrongCredentialsAreReported() async {
        let fake = FakeServer(routes: ["POST /api/v1/login": .json(401, #"{"error":"Invalid credentials"}"#)])
        await #expect(throws: AccountSession.Problem.wrongCredentials) {
            try await AccountSession(session: fake.session)
                .signIn(at: address, username: "admin", password: "wrong")
        }
    }

    @Test func tooManyAttemptsAreReported() async {
        let fake = FakeServer(routes: ["POST /api/v1/login": .json(429, #"{"error":"Too many attempts. Try again in a minute."}"#)])
        await #expect(throws: AccountSession.Problem.tooManyAttempts("Too many attempts. Try again in a minute.")) {
            try await AccountSession(session: fake.session)
                .signIn(at: address, username: "admin", password: "wrong")
        }
    }

    @Test func aRefusedPasswordKeepsTheServersWords() async {
        let fake = FakeServer(routes: ["POST /api/v1/setup": .json(400, #"{"error":"Use a password between 12 and 1024 bytes"}"#)])
        await #expect(throws: AccountSession.Problem.refused("Use a password between 12 and 1024 bytes")) {
            try await AccountSession(session: fake.session)
                .createAdministrator(at: address, username: "admin", password: "short")
        }
    }

    @Test func noAnswerWhileSigningInIsReported() async {
        let fake = FakeServer(routes: [:], failures: ["http://192.168.1.20:8080": URLError(.cannotConnectToHost)])
        await #expect(throws: AccountSession.Problem.noAnswer) {
            try await AccountSession(session: fake.session)
                .signIn(at: address, username: "admin", password: "correct horse")
        }
    }

    @Test func aSignedInRequestCarriesTheBearerToken() async throws {
        let fake = FakeServer(routes: ["GET /api/v1/me": .json(200, #"{"id":"u1","username":"admin","admin":true}"#)])
        let user = try await AccountSession(session: fake.session)
            .currentUser(at: address, token: "session-token-1")
        #expect(user.username == "admin")
        #expect(fake.requests.first?.authorization == "Bearer session-token-1")
    }

    @Test func signingOutEndsTheSessionOnTheServer() async throws {
        let fake = FakeServer(routes: ["POST /api/v1/logout": .text(204, "")])
        try await AccountSession(session: fake.session).signOut(at: address, token: "session-token-1")
        let request = try #require(fake.requests.first)
        #expect(request.line == "POST /api/v1/logout")
        #expect(request.authorization == "Bearer session-token-1")
        #expect(request.header("x-hito-request") == "1")
    }

    @Test func signingOutOfAnAlreadyEndedSessionSucceeds() async throws {
        let fake = FakeServer(routes: ["POST /api/v1/logout": .json(401, #"{"error":"Sign in again"}"#)])
        try await AccountSession(session: fake.session).signOut(at: address, token: "expired")
    }

    @Test func anExpiredSessionIsReported() async {
        let fake = FakeServer(routes: ["GET /api/v1/me": .json(401, #"{"error":"Sign in again"}"#)])
        await #expect(throws: AccountSession.Problem.signedOut) {
            try await AccountSession(session: fake.session)
                .currentUser(at: address, token: "expired")
        }
    }
}
