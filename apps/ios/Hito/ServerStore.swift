import Foundation
import HitoKit
import Security

struct RecentServer: Codable, Equatable, Identifiable {
    let address: ServerAddress
    var username: String?
    var displayName: String?
    var admin: Bool?
    var used: Date

    var id: URL { address.origin }
}

struct ServerStore {
    private let defaults = UserDefaults.standard
    private let recentKey = "servers.recent"
    private let acceptedKey = "servers.unencrypted.accepted"
    private let service = "app.hito.session"

    var recent: [RecentServer] {
        defaults.data(forKey: recentKey).flatMap { try? JSONDecoder().decode([RecentServer].self, from: $0) } ?? []
    }

    var current: RecentServer? { recent.first }

    func use(_ address: ServerAddress, user: User? = nil) {
        var list = recent
        let previous = list.first { $0.address == address }
        list.removeAll { $0.address == address }
        list.insert(
            RecentServer(
                address: address,
                username: user?.username ?? previous?.username,
                displayName: user.map { $0.displayName } ?? previous?.displayName,
                admin: user?.admin ?? previous?.admin,
                used: .now
            ),
            at: 0
        )
        save(Array(list.prefix(5)))
    }

    func forgetAccount(on address: ServerAddress) {
        save(recent.map { server in
            guard server.address == address else { return server }
            return RecentServer(address: server.address, username: nil, displayName: nil, admin: nil, used: server.used)
        })
        forgetToken(for: address)
    }

    func forget(_ address: ServerAddress) {
        save(recent.filter { $0.address != address })
        forgetToken(for: address)
        var accepted = acceptedUnencrypted
        accepted.remove(address.origin.absoluteString)
        defaults.set(Array(accepted), forKey: acceptedKey)
    }

    func acceptUnencrypted(_ address: ServerAddress) {
        var accepted = acceptedUnencrypted
        accepted.insert(address.origin.absoluteString)
        defaults.set(Array(accepted), forKey: acceptedKey)
    }

    func needsUnencryptedNotice(_ address: ServerAddress) -> Bool {
        !address.isEncrypted && !acceptedUnencrypted.contains(address.origin.absoluteString)
    }

    func token(for address: ServerAddress) -> String? {
        var query = base(address)
        query[kSecReturnData as String] = true
        query[kSecMatchLimit as String] = kSecMatchLimitOne
        var item: CFTypeRef?
        guard SecItemCopyMatching(query as CFDictionary, &item) == errSecSuccess, let data = item as? Data else {
            return nil
        }
        return String(data: data, encoding: .utf8)
    }

    struct SessionNotKept: Error {}

    func save(token: String, for address: ServerAddress) throws {
        forgetToken(for: address)
        var query = base(address)
        query[kSecValueData as String] = Data(token.utf8)
        query[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
        guard SecItemAdd(query as CFDictionary, nil) == errSecSuccess else { throw SessionNotKept() }
    }

    func forgetToken(for address: ServerAddress) {
        SecItemDelete(base(address) as CFDictionary)
    }

    private var acceptedUnencrypted: Set<String> {
        Set(defaults.stringArray(forKey: acceptedKey) ?? [])
    }

    private func save(_ list: [RecentServer]) {
        if let data = try? JSONEncoder().encode(list) { defaults.set(data, forKey: recentKey) }
    }

    private func base(_ address: ServerAddress) -> [String: Any] {
        [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: address.origin.absoluteString,
        ]
    }
}
