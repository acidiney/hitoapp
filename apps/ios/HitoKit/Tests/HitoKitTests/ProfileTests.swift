import Foundation
import Testing
@testable import HitoKit

@Suite struct ProfileTests {
    let address = try! ServerAddressInput(parsing: "http://192.168.1.20:8080").candidates[0]

    func api(_ fake: FakeServer) -> HitoAPI {
        HitoAPI(address: address, token: "session-token-1", session: fake.session)
    }

    @Test func devicesListTheCurrentOneFirst() async throws {
        let fake = FakeServer(routes: ["GET /api/v1/account/sessions": .json(200, #"[{"id":"s2","client":"Firefox on Linux","created_at":1,"last_seen_at":20,"current":false},{"id":"s1","client":"Hito for iPhone","created_at":1,"last_seen_at":10,"current":true}]"#)])
        let devices = try await Profile(api: api(fake)).devices()
        #expect(devices.map(\.id) == ["s1", "s2"])
        #expect(devices.map(\.name) == ["Hito for iPhone", "Firefox on Linux"])
        #expect(devices.first?.current == true)
    }

    @Test func everyRequestNamesTheIOSApp() async throws {
        let fake = FakeServer(routes: ["GET /api/v1/account/sessions": .json(200, "[]")])
        _ = try await Profile(api: api(fake)).devices()
        #expect(fake.requests.first?.header("user-agent")?.hasPrefix("HitoiOS/") == true)
    }

    @Test func aDeviceIsSignedOutByItsId() async throws {
        let fake = FakeServer(routes: ["DELETE /api/v1/account/sessions/s2": .text(204, "")])
        try await Profile(api: api(fake)).signOut(device: "s2")
        #expect(fake.requests.map(\.line) == ["DELETE /api/v1/account/sessions/s2"])
    }

    @Test func aBlankNameClearsTheDisplayName() async throws {
        let fake = FakeServer(routes: ["PUT /api/v1/account/display-name": .json(200, #"{"id":"u1","username":"tester","display_name":null,"admin":true}"#)])
        _ = try await Profile(api: api(fake)).rename("   ")
        #expect(fake.requests.first?.object?["display_name"] is NSNull)
    }

    @Test func aPhotoIsSentAsItsBytesAndComesBackVersioned() async throws {
        let fake = FakeServer(routes: ["PUT /api/v1/account/avatar": .json(200, #"{"id":"u1","username":"tester","display_name":null,"admin":true,"avatar_version":"v7"}"#)])
        let bytes = Data([0xFF, 0xD8, 0xFF, 0xE0])
        let user = try await Profile(api: api(fake)).setPhoto(bytes)
        #expect(user.avatarVersion == "v7")
        #expect(fake.requests.first?.body == bytes)
        #expect(fake.requests.first?.header("content-type") == "application/octet-stream")
        #expect(fake.requests.first?.header("x-hito-request") == "1")
        #expect(Profile.photoPath(of: user) == ["api", "v1", "accounts", "u1", "avatar"])
    }

    @Test func aPhotoThatIsTooBigOrNotAPictureSaysSo() async throws {
        let fake = FakeServer(routes: ["PUT /api/v1/account/avatar": .json(413, #"{"error":"too large"}"#)])
        await #expect(throws: Profile.PhotoProblem.tooBig) { try await Profile(api: api(fake)).setPhoto(Data([1])) }
        let other = FakeServer(routes: ["PUT /api/v1/account/avatar": .json(422, #"{"error":"unsupported"}"#)])
        await #expect(throws: Profile.PhotoProblem.notAPicture) { try await Profile(api: api(other)).setPhoto(Data([1])) }
        #expect(Profile.PhotoProblem.tooBig.message == "That photo is over 2 MB. Choose a smaller one.")
    }

    @Test func removingThePhotoClearsItsVersion() async throws {
        let fake = FakeServer(routes: ["DELETE /api/v1/account/avatar": .json(200, #"{"id":"u1","username":"tester","display_name":null,"admin":true,"avatar_version":null}"#)])
        let user = try await Profile(api: api(fake)).removePhoto()
        #expect(user.avatarVersion == nil)
    }

    @Test func theSourceOrderMovesAndSavesWithItsRevision() async throws {
        let order = #"{"revision":4,"fallback":true,"entries":[{"kind":"local_files"},{"kind":"saved_copies"},{"kind":"provider","installation_id":"i1"}]}"#
        let fake = FakeServer(routes: ["GET /api/v1/source-preferences": .json(200, order), "PUT /api/v1/source-preferences": .json(200, order)])
        var current = try await SourceOrder(api: api(fake)).load()
        #expect(current.entries == [.localFiles, .savedCopies, .provider("i1")])
        current.move(2, by: -1)
        _ = try await SourceOrder(api: api(fake)).save(current)
        let body = try #require(fake.requests.last?.object)
        #expect(body["revision"] as? Int == 4)
        #expect((body["entries"] as? [[String: String]])?.map { $0["kind"] ?? "" } == ["local_files", "provider", "saved_copies"])
    }

    @Test func aNewPasswordIsCheckedBeforeItIsSent() {
        #expect(PasswordChange.problem(new: "short", again: "short") == "Use a new password of at least 12 characters.")
        #expect(PasswordChange.problem(new: "long enough phrase", again: "long enough phrasf") == "The new passwords don't match.")
        #expect(PasswordChange.problem(new: "long enough phrase", again: "long enough phrase") == nil)
    }

    @Test func changingThePasswordSaysHowManyOtherDevicesWereSignedOut() async throws {
        let fake = FakeServer(routes: ["POST /api/v1/account/password": .json(200, #"{"revoked":2}"#)])
        let done = try await Profile(api: api(fake)).changePassword(current: "old secret phrase", new: "new secret phrase")
        #expect(done == "Password changed. 2 other devices were signed out.")
        let body = try #require(fake.requests.first?.json)
        #expect(body == ["current_password": "old secret phrase", "new_password": "new secret phrase"])
        let alone = FakeServer(routes: ["POST /api/v1/account/password": .json(200, #"{"revoked":0}"#)])
        #expect(try await Profile(api: api(alone)).changePassword(current: "a", new: "b") == "Password changed.")
    }

    @Test func aWrongCurrentPasswordSaysSo() async {
        let fake = FakeServer(routes: ["POST /api/v1/account/password": .json(422, #"{"code":"wrong_password","message":"Wrong password"}"#)])
        await #expect(throws: PasswordChange.Problem.wrongPassword) {
            try await Profile(api: api(fake)).changePassword(current: "bad", new: "new secret phrase")
        }
    }
}
