import Foundation
import Testing
@testable import HitoKit

@Suite struct SourcePreparationTests {
    let address = try! ServerAddressInput(parsing: "http://192.168.1.20:8080").candidates[0]

    func preparation(_ fake: FakeServer) -> SourcePreparation {
        SourcePreparation(api: HitoAPI(address: address, token: "session-token-1", session: fake.session))
    }

    @Test func preparingAsksOnceWithAFreshRequest() async throws {
        let fake = FakeServer(routes: ["POST /api/v1/recordings/r1/sources/s2/preparation": .json(202, #"{"request_id":"x","preparation_id":"p","recording_id":"r1","source_id":"s2"}"#)])
        try await preparation(fake).start("r1", source: "s2")
        #expect(UUID(uuidString: fake.requests.first?.json?["request_id"] ?? "") != nil)
    }

    @Test func theStatusReadsLikeTheWeb() async throws {
        let fake = FakeServer(routes: ["GET /api/v1/recordings/r1/sources/s2/preparation": .json(200, #"{"preparation_id":"p","recording_id":"r1","source_id":"s2","phase":"preparing","progress_basis_points":4250,"eta_seconds":12,"reason":null,"next_check_at":null,"created_at":1,"updated_at":2}"#)])
        let status = try await preparation(fake).status("r1", source: "s2")
        #expect(status?.message == "Preparing audio…")
        #expect(status?.percent == 42)
        #expect(status?.eta == "Estimated time remaining: 12 seconds.")
        #expect(status?.finished == false)
    }

    @Test func aBlockedPreparationSaysWhy() async throws {
        let fake = FakeServer(routes: ["GET /api/v1/recordings/r1/sources/s2/preparation": .json(200, #"{"preparation_id":"p","recording_id":"r1","source_id":"s2","phase":"blocked","progress_basis_points":null,"eta_seconds":null,"reason":"authorization_required","next_check_at":null,"created_at":1,"updated_at":2}"#)])
        let status = try await preparation(fake).status("r1", source: "s2")
        #expect(status?.message == "Audio preparation is blocked.")
        #expect(status?.reason == "Reconnect your music service to continue.")
        #expect(status?.finished == true)
    }

    @Test func noPreparationYetIsNotAnError() async throws {
        let fake = FakeServer(routes: ["GET /api/v1/recordings/r1/sources/s2/preparation": .json(404, #"{"error":"Resource not found"}"#)])
        #expect(try await preparation(fake).status("r1", source: "s2") == nil)
    }
}
