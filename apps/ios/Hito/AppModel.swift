import Foundation
import HitoKit
import Observation
import OSLog

@MainActor
@Observable
final class AppModel {
    enum Step: Equatable {
        case opening
        case address
        case unencrypted(FoundServer)
        case createAdministrator(ServerAddress)
        case signIn(ServerAddress, reason: String?)
        case unreachable(ServerAddress)
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
            step = .address
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
        step = .address
    }

    func username(for address: ServerAddress) -> String? {
        store.recent.first { $0.address == address }?.username
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
        store.use(address, username: signedIn.user.username)
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
            step = .signedIn(address, try await account.currentUser(at: address, token: token))
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
