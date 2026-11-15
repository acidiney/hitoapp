import Foundation
import Testing
@testable import HitoKit

@Suite struct ArtworkRetrievalTests {
    let address = try! ServerAddressInput(parsing: "http://192.168.1.20:8080").candidates[0]

    func retrieval(_ fake: FakeServer) -> ArtworkRetrieval {
        ArtworkRetrieval(api: HitoAPI(address: address, token: "session-token-1", session: fake.session)) { _ in }
    }

    func receipt(_ status: String, kind: String = "recording_id", id: String = "r1") -> String {
        #"{"id":"job1","\#(kind)":"\#(id)","status":"\#(status)","attempts":1,"created_at":1,"updated_at":2}"#
    }

    @Test func aCoverPathNamesWhatToFetch() {
        #expect(ArtworkRetrieval.Subject(path: ["api", "v1", "recordings", "r1", "artwork"]) == .init(kind: .recording, id: "r1"))
        #expect(ArtworkRetrieval.Subject(path: ["api", "v1", "releases", "a1", "artwork"]) == .init(kind: .release, id: "a1"))
        #expect(ArtworkRetrieval.Subject(path: ["api", "v1", "playlists", "p1", "artwork"]) == .init(kind: .playlist, id: "p1"))
        #expect(ArtworkRetrieval.Subject(path: ["api", "v1", "podcasts", "s1", "artwork"]) == nil)
    }

    @Test func aMissingCoverIsFetchedAndFollowedUntilReady() async throws {
        let fake = FakeServer(routes: [
            "GET /api/v1/recordings/r1/artwork/availability": .json(200, #"{"can_fetch":true}"#),
            "POST /api/v1/recordings/r1/artwork/fetch": .json(202, receipt("queued")),
            "GET /api/v1/artwork-fetches/job1": .sequence([.json(200, receipt("running")), .json(200, receipt("ready"))]),
        ])
        let outcome = try await retrieval(fake).retrieve(.init(kind: .recording, id: "r1"))
        #expect(outcome == .ready)
        #expect(fake.requests.map(\.line).filter { $0.contains("artwork-fetches") }.count == 2)
    }

    @Test func noProviderMeansTheCoverIsUnavailableWithoutAFetch() async throws {
        let fake = FakeServer(routes: ["GET /api/v1/releases/a1/artwork/availability": .json(200, #"{"can_fetch":false}"#)])
        #expect(try await retrieval(fake).retrieve(.init(kind: .release, id: "a1")) == .unavailable)
        #expect(!fake.requests.contains { $0.line.contains("/fetch") })
    }

    @Test func aFailedFetchIsReportedAsFailed() async throws {
        let fake = FakeServer(routes: [
            "GET /api/v1/playlists/p1/artwork/availability": .json(200, #"{"can_fetch":true}"#),
            "POST /api/v1/playlists/p1/artwork/fetch": .json(202, receipt("failed", kind: "playlist_id", id: "p1")),
        ])
        #expect(try await retrieval(fake).retrieve(.init(kind: .playlist, id: "p1")) == .failed)
    }

    @Test func aBusyServerIsAskedAgainAfterAPause() async throws {
        let fake = FakeServer(routes: [
            "GET /api/v1/recordings/r1/artwork/availability": .json(200, #"{"can_fetch":true}"#),
            "POST /api/v1/recordings/r1/artwork/fetch": .sequence([
                .json(429, #"{"error":"busy","code":"artwork_admission_busy"}"#),
                .json(202, receipt("ready")),
            ]),
        ])
        #expect(try await retrieval(fake).retrieve(.init(kind: .recording, id: "r1")) == .ready)
    }

    @Test func aReceiptForAnotherCoverIsRejected() async throws {
        let fake = FakeServer(routes: [
            "GET /api/v1/recordings/r1/artwork/availability": .json(200, #"{"can_fetch":true}"#),
            "POST /api/v1/recordings/r1/artwork/fetch": .json(202, receipt("ready", id: "r2")),
        ])
        #expect(try await retrieval(fake).retrieve(.init(kind: .recording, id: "r1")) == .failed)
    }
}
