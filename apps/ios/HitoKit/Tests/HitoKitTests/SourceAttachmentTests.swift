import Foundation
import Testing
@testable import HitoKit

@Suite struct SourceAttachmentTests {
    let address = try! ServerAddressInput(parsing: "http://192.168.1.20:8080").candidates[0]

    func attachment(_ fake: FakeServer) -> SourceAttachment {
        SourceAttachment(api: HitoAPI(address: address, token: "session-token-1", session: fake.session))
    }

    let candidate = SourceAttachment.Candidate(installationId: "p1", installationRevision: 2, providerRecordingId: "t9")

    @Test func aMatchIsReviewedBeforeItIsAdded() async throws {
        let fake = FakeServer(routes: ["POST /api/v1/recordings/r1/sources/review": .json(200, #"{"review_stamp":"stamp-1","candidate":{"id":"t9","title":"Song","artist":"Band","duration_ms":201000,"release":{"id":"a1","title":"Record","artist":"Band"}}}"#)])
        let review = try await attachment(fake).review("r1", candidate)
        #expect(review.stamp == "stamp-1")
        #expect(review.title == "Song")
        #expect(review.album == "Record")
        let body = try #require(fake.requests.first?.object)
        #expect(body["installation_id"] as? String == "p1")
        #expect(body["installation_revision"] as? Int == 2)
        #expect(body["provider_recording_id"] as? String == "t9")
    }

    @Test func addingSendsTheReviewedCandidateAndItsStamp() async throws {
        let recording = #"{"id":"r1","library_id":"l1","title":"Song","inherited_title":"Song","inherited_artist":"Band","artist":"Band","duration_ms":200000,"available":false,"plays_from_services":true,"liked":false,"album":null,"release_id":null}"#
        let fake = FakeServer(routes: ["POST /api/v1/recordings/r1/sources/attach": .json(200, recording)])
        let track = try await attachment(fake).attach("r1", candidate, stamp: "stamp-1")
        #expect(track.playsFromServices)
        let body = try #require(fake.requests.first?.object)
        #expect(body["review_stamp"] as? String == "stamp-1")
        #expect((body["candidate"] as? [String: Any])?["provider_recording_id"] as? String == "t9")
    }

    @Test func aPastedLinkIsLookedUpThroughTheChosenService() async throws {
        let fake = FakeServer(routes: ["POST /api/v1/recordings/r1/sources/lookup": .json(200, #"{"installation_id":"p1","installation_revision":2,"provider_recording_id":"t9"}"#)])
        let found = try await attachment(fake).lookup("r1", installationId: "p1", revision: 2, reference: "  https://service.invalid/track/9  ")
        #expect(found == candidate)
        let body = try #require(fake.requests.first?.object)
        #expect(body["installation_id"] as? String == "p1")
        #expect(body["installation_revision"] as? Int == 2)
        #expect(body["reference"] as? String == "https://service.invalid/track/9")
    }

    @Test func aLinkTheServiceCannotFindSaysSo() async {
        let fake = FakeServer(routes: ["POST /api/v1/recordings/r1/sources/lookup": .json(404, #"{"error":"Not found"}"#)])
        await #expect(throws: SourceAttachment.Problem.linkNotFound) { try await attachment(fake).lookup("r1", installationId: "p1", revision: 2, reference: "https://service.invalid/x") }
        #expect(SourceAttachment.Problem.linkNotFound.message == "The service couldn't find a track at that link. Check the link and try again.")
    }

    @Test func lengthsAreComparedLikeTheWeb() {
        #expect(SourceAttachment.compare(saved: 200_000, candidate: 202_500) == .init(text: "The lengths match.", close: true))
        #expect(SourceAttachment.compare(saved: 200_000, candidate: 230_000) == .init(text: "The candidate is 30 seconds longer than your saved track.", close: false))
        #expect(SourceAttachment.compare(saved: 200_000, candidate: 80_000) == .init(text: "The candidate is 2:00 shorter than your saved track.", close: false))
        #expect(SourceAttachment.compare(saved: 200_000, candidate: nil) == .init(text: "The candidate does not give a length.", close: false))
        #expect(SourceAttachment.compare(saved: nil, candidate: 1000) == nil)
    }
    @Test func aTrackOutsideYourOwnLibraryCannotTakeASource() async {
        let fake = FakeServer(routes: ["POST /api/v1/recordings/r1/sources/review": .json(404, #"{"error":"Resource not found"}"#)])
        await #expect(throws: SourceAttachment.Problem.notYours) { try await attachment(fake).review("r1", candidate) }
        #expect(SourceAttachment.Problem.notYours.message == "Sources can only be added to tracks in your own library.")
    }

    @Test func aMatchAlreadyOnAnotherTrackIsRefused() async {
        let fake = FakeServer(routes: ["POST /api/v1/recordings/r1/sources/review": .json(409, #"{"error":"Conflict"}"#)])
        await #expect(throws: SourceAttachment.Problem.taken) { try await attachment(fake).review("r1", candidate) }
    }

    @Test func aServiceThatNeedsItsCredentialsIsNotReportedAsTaken() async {
        let fake = FakeServer(routes: ["POST /api/v1/recordings/r1/sources/review": .json(409, #"{"error":"Credentials required","code":"plugin_credentials_required","plugin_connection":{"installation_id":"i1","installation_revision":2}}"#)])
        await #expect(throws: HitoAPI.Problem.credentialsRequired) { try await attachment(fake).review("r1", candidate) }
    }
}
