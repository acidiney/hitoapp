import Foundation
import Testing
@testable import HitoKit

@Suite struct StreamingQualityTests {
    let address = try! ServerAddressInput(parsing: "http://192.168.1.20:8080").candidates[0]
    let leaseId = "6f0b7c1e-0000-4000-8000-000000000001"
    var head: String { "HEAD /api/v1/playback/leases/\(leaseId)/audio" }

    func request() -> AudioRequest {
        AudioRequest(
            leaseId: leaseId,
            recordingId: "r1",
            sourceId: "s1",
            sourceLabel: "Local file",
            url: URL(string: "http://192.168.1.20:8080/api/v1/playback/leases/\(leaseId)/audio")!,
            headers: ["Authorization": "Bearer session-token-1"]
        )
    }

    func playback(_ fake: FakeServer) -> Playback {
        Playback(api: HitoAPI(address: address, token: "session-token-1", session: fake.session))
    }

    @Test func theOriginalNeedsNoPreflight() async throws {
        let fake = FakeServer(routes: [:])
        let choice = try await playback(fake).stream(request(), quality: .original, from: 42)
        #expect(choice == StreamChoice(url: request().url, quality: .original, startOffset: 0, fallback: nil))
        #expect(fake.requests.isEmpty)
    }

    @Test func aConvertedLeaseStreamsFromTheTimeTheServerConfirms() async throws {
        let fake = FakeServer(routes: [
            head: .headed(200, "", ["X-Hito-Stream-Quality": "saver", "X-Hito-Stream-Start-Ms": "5000"]),
        ])
        let choice = try await playback(fake).stream(request(), quality: .saver, from: 5.0004)
        #expect(choice.quality == .saver)
        #expect(choice.startOffset == 5)
        #expect(choice.fallback == nil)
        let query = try #require(URLComponents(url: choice.url, resolvingAgainstBaseURL: false)?.queryItems)
        #expect(query == [URLQueryItem(name: "quality", value: "saver"), URLQueryItem(name: "start_ms", value: "5000")])
        let preflight = try #require(fake.requests.last)
        #expect(preflight.query == "quality=saver&start_ms=5000")
        #expect(preflight.authorization == "Bearer session-token-1")
    }

    @Test func anOriginalAnswerFallsBackToTheOriginalWithAReason() async throws {
        let fake = FakeServer(routes: [head: .headed(200, "", ["X-Hito-Stream-Quality": "original"])])
        let choice = try await playback(fake).stream(request(), quality: .high, from: 0)
        #expect(choice == StreamChoice(url: request().url, quality: .original, startOffset: 0, fallback: StreamChoice.originalOnly))
    }

    @Test func aServerThatIgnoresQualityFallsBackToTheOriginal() async throws {
        let fake = FakeServer(routes: [head: .headed(200, "", [:])])
        let choice = try await playback(fake).stream(request(), quality: .high, from: 0)
        #expect(choice.url == request().url)
        #expect(choice.quality == .original)
        #expect(choice.fallback == StreamChoice.originalOnly)
    }

    @Test func aMismatchedQualityIsNotTrusted() async throws {
        let fake = FakeServer(routes: [
            head: .headed(200, "", ["X-Hito-Stream-Quality": "high", "X-Hito-Stream-Start-Ms": "0"]),
        ])
        let choice = try await playback(fake).stream(request(), quality: .saver, from: 0)
        #expect(choice.quality == .original)
        #expect(choice.url == request().url)
    }

    @Test func aRefusedPreflightFallsBackAndSignedOutStops() async throws {
        let busy = FakeServer(routes: [head: .text(409, "")])
        #expect(try await playback(busy).stream(request(), quality: .saver, from: 3).quality == .original)
        let signedOut = FakeServer(routes: [head: .text(401, "")])
        await #expect(throws: HitoAPI.Problem.signedOut) {
            try await playback(signedOut).stream(request(), quality: .saver, from: 0)
        }
        let gone = FakeServer(routes: [head: .text(404, "")])
        await #expect(throws: HitoAPI.Problem.refused(404)) {
            try await playback(gone).stream(request(), quality: .saver, from: 0)
        }
    }

    @Test func eachQualityHasTheSettingsLabel() {
        #expect(StreamingQuality.allCases.map(\.title) == ["Original", "High", "Data saver"])
        #expect(StreamingQuality.allCases.map(\.choice) == ["Original", "High · AAC 256 kbit/s", "Data saver · AAC 128 kbit/s"])
        #expect(StreamingQuality(rawValue: "saver") == .saver)
    }

    @Test func theChoiceIsRememberedPerAccountAndDefaultsToOriginal() {
        let account = "streaming-\(UUID().uuidString)"
        let other = "streaming-\(UUID().uuidString)"
        #expect(StreamingQuality.preferred(account: account) == .original)
        StreamingQuality.setPreferred(.saver, account: account)
        #expect(StreamingQuality.preferred(account: account) == .saver)
        #expect(StreamingQuality.preferred(account: other) == .original)
        StreamingQuality.setPreferred(.original, account: account)
        #expect(StreamingQuality.preferred(account: account) == .original)
    }
}
