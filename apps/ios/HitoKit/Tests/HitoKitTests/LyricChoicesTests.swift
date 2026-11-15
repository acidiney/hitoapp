import Foundation
import Testing
@testable import HitoKit

@Suite struct LyricChoicesTests {
    let address = try! ServerAddressInput(parsing: "http://192.168.1.20:8080").candidates[0]

    func choices(_ fake: FakeServer) -> LyricChoices {
        LyricChoices(api: HitoAPI(address: address, token: "session-token-1", session: fake.session))
    }

    let identities = #"{"recording_id":"r1","items":[{"installation_id":"p1","installation_revision":2,"label":"Lyrics Co","provider_recording":{"id":"t1"},"selectable":true,"selector":{"kind":"identity","identity_id":"i1","identity_revision":3}},{"installation_id":"p2","installation_revision":1,"label":"Tags","provider_recording":{"id":"t2"},"selectable":true,"selector":{"kind":"observation","observation_id":"o1"}}]}"#

    @Test func choicesSayWhereTheyCameFrom() async throws {
        let fake = FakeServer(routes: ["GET /api/v1/recordings/r1/lyrics/identities": .json(200, identities)])
        let found = try await choices(fake).identities("r1")
        #expect(found.map(\.label) == ["Lyrics Co", "Tags"])
        #expect(found.map(\.origin) == ["Chosen by you", "From this track's details"])
        #expect(found.map(\.removable) == [true, false])
    }

    @Test func readingAChoiceSendsItsSelector() async throws {
        let ready = #"{"status":"ready_v2","recording_id":"r1","document_digest":"d","provider_recording":{"id":"t1"},"selector":{"kind":"identity","identity_id":"i1","identity_revision":3},"document":{"recording":{"id":"t1"},"lyrics":{"id":"l","rendition_id":"x","content":{"kind":"anchored","lines":[{"start_ms":1000,"text":"One"},{"start_ms":4000,"text":"Two"}]}}}}"#
        let fake = FakeServer(routes: [
            "GET /api/v1/recordings/r1/lyrics/identities": .json(200, identities),
            "POST /api/v1/recordings/r1/lyrics/identities/read": .json(200, ready),
        ])
        let first = try #require(try await choices(fake).identities("r1").first)
        let result = try await choices(fake).read("r1", first)
        #expect(result == .lines([LyricLine(start: 1, end: 4, text: "One"), LyricLine(start: 4, end: .infinity, text: "Two")], from: "Lyrics Co"))
        let body = try #require(fake.requests.last?.object)
        #expect(body["installation_revision"] as? Int == 2)
        #expect((body["selector"] as? [String: Any])?["identity_id"] as? String == "i1")
    }

    @Test func plainLyricsHaveNoTiming() {
        let lines = LyricChoices.lines(plain: ["a", "b"])
        #expect(lines.allSatisfy { $0.start < 0 })
        #expect(lines.map(\.text) == ["a", "b"])
    }

    @Test func aMatchIsReviewedThenKept() async throws {
        let fake = FakeServer(routes: [
            "GET /api/v1/recordings/r1/lyrics/services": .json(200, #"{"recording_id":"r1","items":[{"installation_id":"p1","installation_revision":2,"label":"Lyrics Co"}]}"#),
            "POST /api/v1/recordings/r1/lyrics/candidates": .json(200, #"{"items":[{"id":"t9","title":"Song","artist":"Band","duration_ms":200000,"release":{"id":"a","title":"Record","artist":"Band"}}],"next_cursor":null}"#),
            "POST /api/v1/recordings/r1/lyrics/identities/review": .json(200, #"{"review_stamp":"s1","candidate":{"id":"t9","title":"Song","artist":"Band","duration_ms":200000,"release":null}}"#),
            "POST /api/v1/recordings/r1/lyrics/identities": .json(200, #"{"installation_id":"p1","installation_revision":2,"label":"Lyrics Co","provider_recording":{"id":"t9"},"selectable":true,"selector":{"kind":"identity","identity_id":"i9","identity_revision":1}}"#),
        ])
        let service = try #require(try await choices(fake).services("r1").first)
        let hits = try await choices(fake).search("r1", service, query: "Song Band")
        #expect(hits.map(\.title) == ["Song"])
        let review = try await choices(fake).review("r1", service, hits[0])
        #expect(review.stamp == "s1")
        let kept = try await choices(fake).keep("r1", service, hits[0], stamp: review.stamp)
        #expect(kept.origin == "Chosen by you")
        let keep = try #require(fake.requests.last?.object)
        #expect(keep["review_stamp"] as? String == "s1")
        #expect((keep["candidate"] as? [String: Any])?["provider_recording_id"] as? String == "t9")
    }

    @Test func aChoiceCanBeRemoved() async throws {
        let fake = FakeServer(routes: ["DELETE /api/v1/recordings/r1/lyrics/identities/i1": .text(204, "")])
        try await choices(fake).remove("r1", identity: "i1")
        #expect(fake.requests.first?.line == "DELETE /api/v1/recordings/r1/lyrics/identities/i1")
    }

    @Test func automaticLyricsComeFromTheFirstService() async throws {
        let ready = #"{"status":"ready_v1","recording_id":"r1","installation_id":"p1","installation_revision":2,"label":"Lyrics Co","document_digest":"d","provider_recording":{"id":"t"},"document":{"recording":{"id":"t"},"lyrics":{"id":"l","rendition_id":"x","lines":[{"start_ms":0,"end_ms":2000,"text":"Hi"}]}}}"#
        let fake = FakeServer(routes: ["POST /api/v1/recordings/r1/lyrics/automatic": .json(200, ready)])
        #expect(try await choices(fake).automatic("r1") == .lines([LyricLine(start: 0, end: 2, text: "Hi")], from: "Lyrics Co"))
        let none = FakeServer(routes: ["POST /api/v1/recordings/r1/lyrics/automatic": .json(200, #"{"status":"not_found"}"#)])
        #expect(try await choices(none).automatic("r1") == .none)
    }

    @Test func theAutomaticSwitchIsOnUntilTurnedOffPerAccount() {
        let defaults = UserDefaults(suiteName: "lyrics-auto-\(UUID().uuidString)")!
        #expect(LyricChoices.automaticEnabled(defaults: defaults, account: "a1"))
        LyricChoices.setAutomatic(false, defaults: defaults, account: "a1")
        #expect(!LyricChoices.automaticEnabled(defaults: defaults, account: "a1"))
        #expect(LyricChoices.automaticEnabled(defaults: defaults, account: "a2"))
    }
    @Test func yourChoiceComesFirstThenAutomaticLyrics() async throws {
        let ready = #"{"status":"ready_v2","document":{"lyrics":{"id":"l","rendition_id":"x","content":{"kind":"plain","lines":["Mine"]}}}}"#
        let chosen = FakeServer(routes: [
            "GET /api/v1/recordings/r1/lyrics/identities": .json(200, identities),
            "POST /api/v1/recordings/r1/lyrics/identities/read": .json(200, ready),
        ])
        #expect(try await choices(chosen).best("r1", automatic: true) == .lines(LyricChoices.lines(plain: ["Mine"]), from: "Lyrics Co"))
        let auto = #"{"status":"ready_v2","label":"Auto Co","document":{"lyrics":{"id":"l","rendition_id":"x","content":{"kind":"plain","lines":["Auto"]}}}}"#
        let nothing = FakeServer(routes: [
            "GET /api/v1/recordings/r1/lyrics/identities": .json(200, #"{"recording_id":"r1","items":[]}"#),
            "POST /api/v1/recordings/r1/lyrics/automatic": .json(200, auto),
        ])
        #expect(try await choices(nothing).best("r1", automatic: true) == .lines(LyricChoices.lines(plain: ["Auto"]), from: "Auto Co"))
        #expect(try await choices(nothing).best("r1", automatic: false) == .none)
    }
}
