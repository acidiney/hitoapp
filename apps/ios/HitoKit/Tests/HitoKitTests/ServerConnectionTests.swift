import Foundation
import Testing
@testable import HitoKit

@Suite struct ServerConnectionTests {
    let healthy: [String: FakeServer.Answer] = [
        "GET /health": .json(200, #"{"ready":true}"#),
        "GET /api/v1/setup": .json(200, #"{"needs_setup":false}"#),
    ]

    @Test func aNewServerAsksForTheAdministrator() async throws {
        let fake = FakeServer(routes: [
            "GET https://hito.home:8080/health": .json(200, #"{"ready":true}"#),
            "GET https://hito.home:8080/api/v1/setup": .json(200, #"{"needs_setup":true}"#),
        ])
        let found = try await ServerConnection(session: fake.session).find(try ServerAddressInput(parsing: "hito.home:8080"))
        #expect(found.next == .createAdministrator)
        #expect(found.address.origin.absoluteString == "https://hito.home:8080")
    }

    @Test func aServerThatIsSetUpAsksToSignIn() async throws {
        let fake = FakeServer(routes: healthy)
        let found = try await ServerConnection(session: fake.session).find(try ServerAddressInput(parsing: "https://music.example.com"))
        #expect(found.next == .signIn)
    }

    @Test func withoutASchemeHttpIsTriedOnlyWhenHttpsDoesNotAnswer() async throws {
        let fake = FakeServer(
            routes: [
                "GET http://192.168.1.20:8080/health": .json(200, #"{"ready":true}"#),
                "GET http://192.168.1.20:8080/api/v1/setup": .json(200, #"{"needs_setup":false}"#),
            ],
            failures: ["https://192.168.1.20:8080": URLError(.cannotConnectToHost)]
        )
        let found = try await ServerConnection(session: fake.session).find(try ServerAddressInput(parsing: "192.168.1.20:8080"))
        #expect(found.address.origin.absoluteString == "http://192.168.1.20:8080")
        #expect(fake.requests.map(\.line) == [
            "GET https://192.168.1.20:8080/health",
            "GET http://192.168.1.20:8080/health",
            "GET http://192.168.1.20:8080/api/v1/setup",
        ])
    }

    @Test func aPlainHttpServerAnsweringOnHttpsFallsBackToHttp() async throws {
        let fake = FakeServer(
            routes: [
                "GET http://localhost:18091/health": .json(200, #"{"ready":true}"#),
                "GET http://localhost:18091/api/v1/setup": .json(200, #"{"needs_setup":true}"#),
            ],
            failures: ["https://localhost:18091": URLError(.secureConnectionFailed)]
        )
        let found = try await ServerConnection(session: fake.session).find(try ServerAddressInput(parsing: "localhost:18091"))
        #expect(found.address.origin.absoluteString == "http://localhost:18091")
        #expect(found.next == .createAdministrator)
    }

    @Test func aCertificateProblemOnHttpsIsNeverReplacedByHttp() async {
        let fake = FakeServer(routes: healthy, failures: ["https://hito.home": URLError(.serverCertificateUntrusted)])
        await #expect(throws: ServerConnection.Problem.certificate) {
            try await ServerConnection(session: fake.session).find(try ServerAddressInput(parsing: "hito.home"))
        }
        #expect(fake.requests.map(\.line) == ["GET https://hito.home/health"])
    }

    @Test func aTypedHttpsAddressIsNeverRetriedOverHttp() async {
        let fake = FakeServer(routes: healthy, failures: ["https://hito.home": URLError(.cannotConnectToHost)])
        await #expect(throws: ServerConnection.Problem.noAnswer) {
            try await ServerConnection(session: fake.session).find(try ServerAddressInput(parsing: "https://hito.home"))
        }
        #expect(fake.requests.count == 1)
    }

    @Test func somethingElseAnsweringIsNotHito() async {
        let fake = FakeServer(routes: ["GET https://192.168.1.1/health": .text(200, "OK")])
        await #expect(throws: ServerConnection.Problem.notHito) {
            try await ServerConnection(session: fake.session).find(try ServerAddressInput(parsing: "https://192.168.1.1"))
        }
    }

    @Test func aMissingSetupEndpointIsNotHito() async {
        let fake = FakeServer(routes: [
            "GET https://hito.home/health": .json(200, #"{"ready":true}"#),
            "GET https://hito.home/api/v1/setup": .text(404, "missing"),
        ])
        await #expect(throws: ServerConnection.Problem.notHito) {
            try await ServerConnection(session: fake.session).find(try ServerAddressInput(parsing: "https://hito.home"))
        }
    }

    @Test func aRedirectToAnotherHostIsNotFollowed() async {
        let fake = FakeServer(routes: [
            "GET https://hito.home/health": .redirect("https://elsewhere.example.com/health"),
            "GET https://elsewhere.example.com/health": .json(200, #"{"ready":true}"#),
        ])
        await #expect(throws: ServerConnection.Problem.notHito) {
            try await ServerConnection(session: fake.session).find(try ServerAddressInput(parsing: "https://hito.home"))
        }
        #expect(!fake.requests.contains { $0.line.contains("elsewhere") })
    }

    @Test func noAnswerOnEitherSchemeIsReported() async {
        let fake = FakeServer(routes: [:], failures: [
            "https://hito.home:8080": URLError(.timedOut),
            "http://hito.home:8080": URLError(.cannotFindHost),
        ])
        await #expect(throws: ServerConnection.Problem.noAnswer) {
            try await ServerConnection(session: fake.session).find(try ServerAddressInput(parsing: "hito.home:8080"))
        }
    }

    @Test func aServerStillStartingIsReported() async {
        let fake = FakeServer(routes: ["GET https://hito.home/health": .json(200, #"{"ready":false}"#)])
        await #expect(throws: ServerConnection.Problem.notReady) {
            try await ServerConnection(session: fake.session).find(try ServerAddressInput(parsing: "https://hito.home"))
        }
    }

    @Test func checkingAKnownAddressAgainUsesOnlyThatAddress() async throws {
        let address = try #require(try ServerAddressInput(parsing: "http://192.168.1.20:8080").candidates.first)
        let fake = FakeServer(routes: [
            "GET http://192.168.1.20:8080/health": .json(200, #"{"ready":true}"#),
            "GET http://192.168.1.20:8080/api/v1/setup": .json(200, #"{"needs_setup":false}"#),
        ])
        let next = try await ServerConnection(session: fake.session).check(address)
        #expect(next == .signIn)
        #expect(fake.requests.map(\.line) == [
            "GET http://192.168.1.20:8080/health",
            "GET http://192.168.1.20:8080/api/v1/setup",
        ])
    }

    @Test func checkingSendsNoCredentials() async throws {
        let fake = FakeServer(routes: healthy)
        _ = try await ServerConnection(session: fake.session).find(try ServerAddressInput(parsing: "https://hito.home"))
        #expect(fake.requests.allSatisfy { $0.authorization == nil })
    }

    @Test func cancellingStopsTheSearch() async {
        let fake = FakeServer(routes: healthy, failures: ["https://hito.home": URLError(.cancelled)])
        await #expect(throws: CancellationError.self) {
            try await ServerConnection(session: fake.session).find(try ServerAddressInput(parsing: "hito.home"))
        }
        #expect(fake.requests.count == 1)
    }
}
