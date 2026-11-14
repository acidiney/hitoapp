import Foundation
import HitoKit
import Observation
import OSLog

@MainActor
@Observable
final class AppModel {
    enum Step: Equatable {
        case opening
        case choose
        case thisPhone
        case address
        case unencrypted(FoundServer)
        case createAdministrator(ServerAddress)
        case signIn(ServerAddress, reason: String?)
        case unreachable(ServerAddress)
        case locked(ServerAddress, User)
        case signedIn(ServerAddress, User)
    }

    private(set) var step: Step = .opening
    private(set) var recent: [RecentServer] = []
    private let connection = ServerConnection()
    private let account = AccountSession()
    private let store = ServerStore()
    private let log = Logger(subsystem: "app.hito", category: "connection")
    private var reconnecting: Task<Void, Never>?

    func open() async {
        recent = store.recent
        guard let server = store.current else {
            step = .choose
            return
        }
        await reopen(server.address)
    }

    func find(_ input: ServerAddressInput) async throws {
        let found = try await connection.find(input)
        log.info("connection.found")
        proceed(found)
    }

    func choose(_ server: RecentServer) async throws {
        let next = try await connection.check(server.address)
        proceed(FoundServer(address: server.address, next: next))
    }

    func connectAnyway(_ found: FoundServer) {
        store.acceptUnencrypted(found.address)
        proceed(found)
    }

    func changeAddress() {
        step = .address
    }

    func chooseServer() {
        step = .address
    }

    func chooseThisPhone() {
        step = .thisPhone
    }

    func backToChoice() {
        step = .choose
    }

    func forget(_ server: RecentServer) {
        store.forget(server.address)
        recent = store.recent
    }

    func createAdministrator(at address: ServerAddress, username: String, password: String) async throws {
        finish(try await account.createAdministrator(at: address, username: username, password: password), at: address)
    }

    func signIn(at address: ServerAddress, username: String, password: String) async throws {
        finish(try await account.signIn(at: address, username: username, password: password), at: address)
    }

    func signOut() {
        guard case let .signedIn(address, _) = step else { return }
        let token = store.token(for: address)
        store.forgetToken(for: address)
        step = .signIn(address, reason: "You signed out.")
        guard let token else { return }
        Task { [account, log] in
            do {
                try await account.signOut(at: address, token: token)
            } catch {
                log.info("connection.sign_out_unconfirmed")
            }
        }
    }

    func changeServer() {
        reconnecting?.cancel()
        recent = store.recent
        step = .choose
    }

    func remembered(on address: ServerAddress) -> RecentServer? {
        store.recent.first { $0.address == address && $0.username != nil }
    }

    func forgetMe(on address: ServerAddress) {
        store.forgetAccount(on: address)
        recent = store.recent
    }

    func checkLock() async {
        guard case let .signedIn(address, user) = step, let token = store.token(for: address) else { return }
        if let status = try? await account.lockStatus(at: address, token: token), status.locked {
            step = .locked(address, user)
        }
    }

    func unlock(pin: String) async throws -> Bool {
        guard case let .locked(address, user) = step, let token = store.token(for: address) else { return false }
        switch try await account.unlock(at: address, token: token, pin: pin) {
        case .unlocked:
            step = .signedIn(address, user)
            return true
        case .wrongPin:
            return false
        case .signedOut:
            store.forgetToken(for: address)
            step = .signIn(address, reason: "Too many wrong PINs. Sign in again.")
            return true
        }
    }

    func signOutWhileLocked() {
        guard case let .locked(address, user) = step else { return }
        step = .signedIn(address, user)
        signOut()
    }

    private func proceed(_ found: FoundServer) {
        if store.needsUnencryptedNotice(found.address) {
            step = .unencrypted(found)
            return
        }
        store.use(found.address)
        recent = store.recent
        step = found.next == .createAdministrator ? .createAdministrator(found.address) : .signIn(found.address, reason: nil)
    }

    private func finish(_ signedIn: SignedIn, at address: ServerAddress) {
        store.save(token: signedIn.token, for: address)
        store.use(address, user: signedIn.user)
        recent = store.recent
        log.info("connection.signed_in")
        step = .signedIn(address, signedIn.user)
    }

    private func reopen(_ address: ServerAddress) async {
        guard let token = store.token(for: address) else {
            await checkThenSignIn(address)
            return
        }
        do {
            let user = try await account.currentUser(at: address, token: token)
            step = .signedIn(address, user)
            await checkLock()
        } catch AccountSession.Problem.locked {
            if let server = store.recent.first(where: { $0.address == address }), let username = server.username {
                step = .locked(address, User(id: "", username: username, admin: server.admin ?? false, displayName: server.displayName))
            } else {
                step = .signIn(address, reason: nil)
            }
        } catch AccountSession.Problem.signedOut {
            store.forgetToken(for: address)
            step = .signIn(address, reason: "This device was signed out. Sign in again.")
        } catch {
            waitForServer(address)
        }
    }

    private func checkThenSignIn(_ address: ServerAddress) async {
        do {
            let next = try await connection.check(address)
            step = next == .createAdministrator ? .createAdministrator(address) : .signIn(address, reason: nil)
        } catch {
            waitForServer(address)
        }
    }

    private func waitForServer(_ address: ServerAddress) {
        log.info("connection.unreachable_at_open")
        step = .unreachable(address)
        reconnecting?.cancel()
        reconnecting = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(for: .seconds(10))
                guard let self, !Task.isCancelled, case .unreachable(address) = self.step else { return }
                if (try? await self.connection.check(address)) != nil {
                    await self.reopen(address)
                    return
                }
            }
        }
    }
}
