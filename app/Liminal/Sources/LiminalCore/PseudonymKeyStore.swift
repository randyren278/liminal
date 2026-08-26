import Foundation
import Security

/// Master plan §18: `pseudonym_hmac_key` is a Keychain entry, not a config value or a per-process
/// random key. A per-process key would defeat §39/§40's whole point -- "recurring proximity
/// cluster" detection requires the SAME peripheral to hash to the SAME pseudonym across restarts.
/// A per-process key would also defeat itself in the opposite direction: without persistence,
/// "recurring" would silently and incorrectly reset every time the app restarts, and nothing in
/// the UI would reveal that the history had been invisibly discarded.
public enum PseudonymKeyStoreError: Error {
    case unexpectedStatus(OSStatus)
    case unreadableStoredKey
}

private let keychainService = "com.liminal.pseudonym"
private let keychainAccount = "pseudonym_hmac_key"
private let keyLengthBytes = 32

/// Reads the persisted HMAC key from the Keychain, or generates and stores a new random one if
/// none exists yet. Not unit-tested against the real Keychain (CI environments can't reliably
/// guarantee an unlocked, writable login keychain) -- `hmacSha256Hex` itself, which this key
/// feeds into, is fully tested; only the "where does the key come from" storage step is not.
public func loadOrCreatePseudonymKey() throws -> Data {
    if let existing = try readKey() {
        return existing
    }
    var newKey = Data(count: keyLengthBytes)
    let result = newKey.withUnsafeMutableBytes { SecRandomCopyBytes(kSecRandomDefault, keyLengthBytes, $0.baseAddress!) }
    guard result == errSecSuccess else {
        throw PseudonymKeyStoreError.unexpectedStatus(result)
    }
    try storeKey(newKey)
    return newKey
}

private func readKey() throws -> Data? {
    let query: [String: Any] = [
        kSecClass as String: kSecClassGenericPassword,
        kSecAttrService as String: keychainService,
        kSecAttrAccount as String: keychainAccount,
        kSecReturnData as String: true,
    ]
    var item: CFTypeRef?
    let status = SecItemCopyMatching(query as CFDictionary, &item)
    switch status {
    case errSecSuccess:
        guard let data = item as? Data else { throw PseudonymKeyStoreError.unreadableStoredKey }
        return data
    case errSecItemNotFound:
        return nil
    default:
        throw PseudonymKeyStoreError.unexpectedStatus(status)
    }
}

private func storeKey(_ key: Data) throws {
    let query: [String: Any] = [
        kSecClass as String: kSecClassGenericPassword,
        kSecAttrService as String: keychainService,
        kSecAttrAccount as String: keychainAccount,
        kSecValueData as String: key,
    ]
    let status = SecItemAdd(query as CFDictionary, nil)
    guard status == errSecSuccess else {
        throw PseudonymKeyStoreError.unexpectedStatus(status)
    }
}
