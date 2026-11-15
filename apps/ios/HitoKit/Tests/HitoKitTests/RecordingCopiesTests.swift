import Foundation
import Testing
@testable import HitoKit

@Suite struct RecordingCopiesTests {
    let address = try! ServerAddressInput(parsing: "http://192.168.1.20:8080").candidates[0]

    func copies(_ fake: FakeServer) -> RecordingCopies {
        RecordingCopies(api: HitoAPI(address: address, token: "session-token-1", session: fake.session))
    }

    @Test func copiesSayHowHitoKnowsTheyMatch() async throws {
        let body = #"{"canonical_id":"r1","copies":[{"recording_id":"r1","title":"Song","artist":"Band","album":"Record","available":true,"canonical":true,"proof":null,"sources":[{"id":"s1","recording_id":"r1","provider":"local","label":"Song.flac","ready":true,"can_prepare":false}]},{"recording_id":"r2","title":"Song","artist":"Band","album":null,"available":false,"canonical":false,"proof":"isrc","sources":[]}]}"#
        let fake = FakeServer(routes: ["GET /api/v1/recordings/r1/copies": .json(200, body)])
        let found = try await copies(fake).load("r1")
        #expect(found.map(\.id) == ["r1", "r2"])
        #expect(found.map(\.line) == ["Band · Record", "Band"])
        #expect(found.map(\.audio) == ["Audio ready", "No audio ready"])
        #expect(found.map(\.proof) == ["Shown in your lists", "Same ISRC"])
        #expect(found.map(\.canSeparate) == [false, true])
        #expect(found.first?.sources == ["Song.flac · ready"])
    }

    @Test func separatingACopyAsksTheServer() async throws {
        let fake = FakeServer(routes: ["POST /api/v1/recordings/r2/copies/split": .json(200, #"{"recording_id":"r2","separate":true}"#)])
        #expect(try await copies(fake).separate("r2"))
    }

    @Test func creditsJoinLikeTheWeb() {
        let credits = [TrackMetadata.Credit(name: "A", join: " feat. "), TrackMetadata.Credit(name: "B", join: nil), TrackMetadata.Credit(name: "C", join: nil)]
        #expect(TrackMetadata.creditLine(credits) == "A feat. B, C")
        #expect(TrackMetadata.creditLine([]) == nil)
    }
}
