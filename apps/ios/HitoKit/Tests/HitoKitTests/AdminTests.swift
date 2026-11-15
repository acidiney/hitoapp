import Foundation
import Testing
@testable import HitoKit

@Suite struct AdminTests {
    let address = try! ServerAddressInput(parsing: "http://192.168.1.20:8080").candidates[0]

    func admin(_ fake: FakeServer) -> Admin {
        Admin(api: HitoAPI(address: address, token: "session-token-1", session: fake.session))
    }

    @Test func accountsSayWhoTheyAreAndWhenTheyWereActive() async throws {
        let now = Date(timeIntervalSince1970: 10_000)
        let fake = FakeServer(routes: ["GET /api/v1/users": .json(200, #"[{"id":"u1","username":"admin","display_name":"Ada","admin":true,"last_active_at":9820,"avatar_version":"p3"},{"id":"u2","username":"guest","display_name":null,"admin":false,"last_active_at":null}]"#)])
        let accounts = try await admin(fake).accounts()
        #expect(accounts.map(\.name) == ["Ada", "guest"])
        #expect(accounts.map(\.avatarVersion) == ["p3", nil])
        #expect(accounts.map { $0.hint(now: now) } == ["admin · Administrator · active 3 minutes ago", "Member · no open sessions"])
    }

    @Test func activityFromTheLastMinuteOrAheadOfTheClockSaysNow() async throws {
        let now = Date(timeIntervalSince1970: 10_000)
        let fake = FakeServer(routes: ["GET /api/v1/users": .json(200, #"[{"id":"u1","username":"admin","display_name":null,"admin":true,"last_active_at":10002},{"id":"u2","username":"guest","display_name":null,"admin":false,"last_active_at":9970}]"#)])
        let accounts = try await admin(fake).accounts()
        #expect(accounts.map { $0.hint(now: now) } == ["Administrator · active now", "Member · active now"])
    }

    @Test func creatingAnAccountSendsItsCredentials() async throws {
        let fake = FakeServer(routes: ["POST /api/v1/users": .json(201, #"{"id":"u3","username":"friend","admin":false,"display_name":null}"#)])
        try await admin(fake).createAccount(username: " friend ", password: "long enough phrase")
        #expect(fake.requests.first?.json == ["username": "friend", "password": "long enough phrase"])
    }

    @Test func deletingAnAccountConfirmsItsUsername() async throws {
        let fake = FakeServer(routes: ["DELETE /api/v1/users/u2": .text(204, "")])
        try await admin(fake).deleteAccount("u2", username: "guest")
        #expect(fake.requests.first?.json == ["username": "guest"])
    }

    @Test func libraryAccessIsGivenAndRemoved() async throws {
        let fake = FakeServer(routes: ["PUT /api/v1/libraries/l1/members/u2": .text(204, ""), "DELETE /api/v1/libraries/l1/members/u2": .text(204, "")])
        try await admin(fake).setAccess(library: "l1", account: "u2", to: true)
        try await admin(fake).setAccess(library: "l1", account: "u2", to: false)
        #expect(fake.requests.map(\.line) == ["PUT /api/v1/libraries/l1/members/u2", "DELETE /api/v1/libraries/l1/members/u2"])
    }

    @Test func foldersSayWhetherTheyAreReachable() async throws {
        let fake = FakeServer(routes: ["GET /api/v1/libraries/l1/folders": .json(200, #"[{"id":"f1","library_id":"l1","host_device":"server","name":"Music folder","revision":2,"connected":true,"available":true,"last_scan_at":100,"latest_job":null},{"id":"f2","library_id":"l1","host_device":"server","name":"Old drive","revision":1,"connected":true,"available":false,"latest_job":{"id":"j1","library_id":"l1","kind":"scan","state":"running","progress":40}}]"#)])
        let folders = try await admin(fake).folders("l1")
        #expect(folders.map(\.status) == ["read-only", "can't be reached · scanning 40%"])
        #expect(folders.map(\.revision) == [2, 1])
    }

    @Test func scanningAndRemovingFolders() async throws {
        let job = #"{"id":"j2","library_id":"l1","kind":"scan","state":"queued","progress":0}"#
        let fake = FakeServer(routes: [
            "POST /api/v1/folders/f1/scan": .json(202, job),
            "POST /api/v1/libraries/l1/scan": .json(202, "[\(job)]"),
            "DELETE /api/v1/folders/f1": .text(204, ""),
        ])
        try await admin(fake).scan(folder: "f1")
        try await admin(fake).scanAll("l1")
        try await admin(fake).removeFolder("f1", revision: 2)
        #expect(fake.requests.map(\.line) == ["POST /api/v1/folders/f1/scan", "POST /api/v1/libraries/l1/scan", "DELETE /api/v1/folders/f1"])
        #expect(fake.requests.last?.object?["revision"] as? Int == 2)
    }

    @Test func removingMissingTracksSaysWhatWasKept() async throws {
        let fake = FakeServer(routes: ["DELETE /api/v1/libraries/l1/missing-recordings": .json(200, #"{"removed":3,"kept_referenced":1,"more":false}"#)])
        #expect(try await admin(fake).removeMissing("l1") == "Removed 3 tracks. 1 stays because it's in a playlist, liked or downloaded.")
        let none = FakeServer(routes: ["DELETE /api/v1/libraries/l1/missing-recordings": .json(200, #"{"removed":0,"kept_referenced":0,"more":false}"#)])
        #expect(try await admin(none).removeMissing("l1") == "No tracks with missing files.")
    }

    @Test func addingAFolderBrowsesTheServersSharedDirectories() async throws {
        let fake = FakeServer(routes: [
            "GET /api/v1/storage/directories": .json(200, #"[{"id":"d1","name":"Music","available":true},{"id":"d2","name":"Offline","available":false}]"#),
            "POST /api/v1/storage/browse": .json(200, #"{"directory":{"directory_id":"d1","components":[]},"directories":[{"name":"Jazz","directory":{"directory_id":"d1","components":["Jazz"]}}],"next_offset":null}"#),
            "POST /api/v1/libraries/l1/folders": .json(201, #"{"id":"f9","library_id":"l1","host_device":"server","name":"Jazz","revision":1,"connected":true,"available":true}"#),
        ])
        let roots = try await admin(fake).sharedDirectories()
        #expect(roots.map(\.name) == ["Music"])
        let inside = try await admin(fake).browse(roots[0].place)
        #expect(inside.map(\.name) == ["Jazz"])
        try await admin(fake).addFolder(inside[0].place, to: "l1")
        let body = try #require(fake.requests.last?.object)
        let directory = try #require(body["directory"] as? [String: Any])
        #expect(directory["directory_id"] as? String == "d1")
        #expect(directory["components"] as? [String] == ["Jazz"])
    }

    @Test func sharedCorrectionsSayWhatTheyChangedAndWhetherTheyApply() async throws {
        let fake = FakeServer(routes: ["GET /api/v1/artist-corrections": .json(200, #"{"revision":3,"total":2,"offset":0,"limit":25,"items":[{"id":"c1","kind":"split","recordings":1,"retired":0,"applied":true,"revision":2,"promoted_by_me":true},{"id":"c2","kind":"unlink","recordings":4,"retired":2,"applied":false,"revision":5,"promoted_by_me":false}]}"#)])
        let page = try await admin(fake).sharedCorrections(offset: 0)
        #expect(fake.requests.first?.query == "offset=0&limit=25")
        #expect(page.total == 2)
        #expect(page.items.map(\.title) == ["Split · 1 recording", "Unlink · 4 recordings"])
        #expect(page.items.map(\.detail) == [nil, "2 no longer match the file tags"])
        #expect(page.items.map(\.status) == ["Applied", "Withdrawn"])
    }

    @Test func withdrawingASharedCorrectionSendsItsRevisionOnce() async throws {
        let fake = FakeServer(routes: ["POST /api/v1/artist-corrections/c1/undo": .json(200, #"{"request_id":"7B0A62C1-58E4-4D2B-9C59-7C1D1B0D3A11","correction_id":"c1","revision":3,"applied":false,"recordings":1}"#)])
        let request = UUID(uuidString: "7B0A62C1-58E4-4D2B-9C59-7C1D1B0D3A11")!
        let correction = SharedCorrection(id: "c1", kind: .split, recordings: 1, retired: 0, applied: true, revision: 2)
        let note = try await admin(fake).toggle(correction, request: request)
        let body = try #require(fake.requests.first?.object)
        #expect(body["expected_revision"] as? Int == 2)
        #expect((body["request_id"] as? String)?.uppercased() == request.uuidString)
        #expect(note == "The shared correction was withdrawn.")
    }
}

