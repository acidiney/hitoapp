import Foundation
import Testing
@testable import HitoKit

@Suite struct SavedCopiesTests {
    let address = try! ServerAddressInput(parsing: "http://192.168.1.20:8080").candidates[0]

    func copies(_ fake: FakeServer) -> SavedCopies {
        SavedCopies(api: HitoAPI(address: address, token: "session-token-1", session: fake.session))
    }

    func status(_ state: String, bytes: Int = 0, total: String = "null", error: String = "null", matches: Bool = true) -> String {
        #"{"status":"\#(state)","intent_id":"i1","recording_id":"r1","host_device_id":"server","choice_revision":1,"matches_source_choice":\#(matches),"bytes_received":\#(bytes),"total_bytes":\#(total),"error":\#(error),"source_id":"s1"}"#
    }

    @Test func aTrackWithoutACopyHasNone() async throws {
        let fake = FakeServer(routes: ["GET /api/v1/recordings/r1/download": .json(404, #"{"error":"not found"}"#)])
        #expect(try await copies(fake).status("r1") == nil)
    }

    @Test func aCopyInProgressSaysHowFarItIs() async throws {
        let fake = FakeServer(routes: ["GET /api/v1/recordings/r1/download": .json(200, status("running", bytes: 1_000_000, total: "4000000"))])
        let copy = try #require(try await copies(fake).status("r1"))
        #expect(copy.summary == "Saving a copy on your server… 1.0 of 4.0 MB")
        #expect(copy.progress == 0.25)
        #expect(copy.transferring)
    }

    @Test func eachStateHasPlainWording() {
        func copy(_ state: SavedCopy.State, error: String? = nil, matches: Bool = true) -> SavedCopy {
            SavedCopy(intentId: "i1", state: state, error: error, bytesReceived: 0, totalBytes: nil, matchesSourceChoice: matches)
        }
        #expect(copy(.ready).summary == "Saved on your server. Plays from the copy when the service can't.")
        #expect(copy(.ready, matches: false).summary == "A copy from a previous source is saved on your server.")
        #expect(copy(.queued).summary == "Waiting to save a copy.")
        #expect(copy(.queued, error: "insufficient_storage").summary == "Not enough disk space on your server. Hito will try again.")
        #expect(copy(.queued, error: "provider_unavailable").summary == "The music service didn't deliver the audio. Hito will try again.")
        #expect(copy(.verifying).summary == "Checking the saved audio…")
        #expect(copy(.failed).summary == "The copy wasn't saved. Remove it, then try again.")
        #expect(copy(.running).summary == "Saving a copy on your server…")
        #expect(copy(.running).progress == nil)
    }

    @Test func savingSendsARequestIdAndRemovingUsesTheIntent() async throws {
        let request = UUID(uuidString: "7B0A62C1-58E4-4D2B-9C59-7C1D1B0D3A11")!
        let fake = FakeServer(routes: [
            "POST /api/v1/recordings/r1/download": .json(202, #"{"request_id":"7B0A62C1-58E4-4D2B-9C59-7C1D1B0D3A11","intent_id":"i1","recording_id":"r1"}"#),
            "DELETE /api/v1/downloads/i1": .text(204, ""),
        ])
        #expect(try await copies(fake).save("r1", request: request) == "i1")
        try await copies(fake).remove("i1")
        #expect((fake.requests.first?.json?["request_id"])?.uppercased() == request.uuidString)
        #expect(fake.requests.map(\.line) == ["POST /api/v1/recordings/r1/download", "DELETE /api/v1/downloads/i1"])
    }

    @Test func aRefusedCopySaysWhyInTheServersWords() async throws {
        let fake = FakeServer(routes: ["POST /api/v1/recordings/r1/download": .json(409, #"{"error":"This source supports playback only. Downloads are unavailable."}"#)])
        await #expect(throws: SavedCopies.Refused(reason: "This source supports playback only. Downloads are unavailable.")) {
            try await copies(fake).save("r1", request: UUID())
        }
    }
}

