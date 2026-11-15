import Foundation
import Testing
@testable import HitoKit

@Suite struct AudiobooksTests {
    let address = try! ServerAddressInput(parsing: "http://192.168.1.20:8080").candidates[0]

    func audiobooks(_ fake: FakeServer) -> Audiobooks {
        Audiobooks(api: HitoAPI(address: address, token: "session-token-1", session: fake.session))
    }

    func chapter(_ entry: String, _ recording: String, duration: Int) -> String {
        #"{"entry_id":"\#(entry)","position":0,"recording_id":"\#(recording)","title":"Chapter \#(entry)","inherited_title":"Chapter \#(entry)","artist":"Author","inherited_artist":"Author","album":null,"release_id":null,"duration_ms":\#(duration),"available":true,"plays_from_services":false,"liked":false}"#
    }

    @Test func booksArePaged() async throws {
        let fake = FakeServer(routes: ["GET /api/v1/audiobooks/page": .json(200, #"{"items":[{"id":"b1","name":"Fixture Book","kind":"local"}],"pagination":{"limit":24,"next_cursor":"n2","previous_cursor":null}}"#)])
        let page = try await audiobooks(fake).books(cursor: nil)
        #expect(page.books.map(\.name) == ["Fixture Book"])
        #expect(page.next == "n2")
        #expect(fake.requests.first?.query == "limit=24")
    }

    @Test func aBookContinuesWhereYouStopped() async throws {
        let body = #"{"playlist":{"id":"b1","name":"Fixture Book","kind":"local"},"chapters":[\#(chapter("e1", "r1", duration: 600_000)),\#(chapter("e2", "r2", duration: 600_000))],"bookmarks":[{"id":"k1","audiobook_id":"b1","entry_id":"e2","recording_id":"r2","position_ms":90000,"label":"Good part"}],"continuation":{"chapter":{"audiobook_id":"b1","entry_id":"e2","recording_id":"r2"},"progress":{"recording_id":"r2","position_ms":300000,"speed":1.0,"updated_at":1}}}"#
        let fake = FakeServer(routes: ["GET /api/v1/audiobooks/b1": .json(200, body)])
        let book = try await audiobooks(fake).detail("b1")
        #expect(book.chapters.map(\.track.id) == ["r1", "r2"])
        #expect(book.continuation == Audiobooks.Continuation(index: 1, positionMs: 300_000))
        #expect(book.continueLabel == "Continue ch. 2 · 5:00")
        #expect(book.overall == Audiobooks.Overall(fraction: 0.75, left: "5 min left"))
        #expect(book.bookmarks.map(\.label) == ["Good part"])
        #expect(book.bookmarks.first?.chapterIndex == 1)
        #expect(book.anchors.map(\.entryId) == ["e1", "e2"])
    }

    @Test func aBookNotStartedHasNoContinuation() async throws {
        let body = #"{"playlist":{"id":"b1","name":"Fixture Book","kind":"local"},"chapters":[\#(chapter("e1", "r1", duration: 0))],"bookmarks":[],"continuation":null}"#
        let fake = FakeServer(routes: ["GET /api/v1/audiobooks/b1": .json(200, body)])
        let book = try await audiobooks(fake).detail("b1")
        #expect(book.continuation == nil)
        #expect(book.continueLabel == nil)
        #expect(book.overall == nil)
    }

    @Test func creatingAndBookmarkingAreSyncChanges() async throws {
        let fake = FakeServer(routes: ["POST /api/v1/sync": .acknowledging])
        let books = audiobooks(fake)
        try await books.create(from: "p1", device: "d1")
        let anchor = AudiobookAnchor(audiobookId: "b1", entryId: "e2", recordingId: "r2")
        let bookmark = try await books.bookmark(anchor, at: 90_000, label: "Good part", device: "d1")
        try await books.removeBookmark(bookmark, from: "b1", device: "d1")
        let actions = fake.requests.compactMap { (($0.object?["operations"] as? [[String: Any]])?.first?["action"]) as? [String: Any] }
        #expect(actions.map { $0["kind"] as? String } == ["audiobook_create", "audiobook_bookmark_create", "audiobook_bookmark_remove"])
        #expect(actions[0]["playlist_id"] as? String == "p1")
        #expect(actions[1]["position_ms"] as? Int == 90_000)
        #expect(actions[1]["label"] as? String == "Good part")
        #expect(actions[1]["bookmark_id"] as? String == bookmark)
        #expect(actions[2]["bookmark_id"] as? String == bookmark)
    }
}
