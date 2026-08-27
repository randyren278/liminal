import Foundation
import LocalAuthentication
import Security

/// Master plan §18: `pseudonym_hmac_key` is a Keychain entry, not a config value or a per-process
/// random key. A per-process key would defeat §39/§40's whole point -- "recurring proximity
/// cluster" detection requires the SAME peripheral to hash to the SAME pseudonym across restarts.
/// A per-process key would also defeat itself in the opposite direction: without persistence,
/// "recurring" would silently and incorrectly reset every time the app restarts, and nothing in
/// the UI would reveal that the history had been invisibly discarded.
public enum PseudonymKeyStoreError: Error, CustomStringConvertible {
    case unexpectedStatus(OSStatus)
    case unreadableStoredKey
    case interactionRequired

    public var description: String {
        switch self {
        case let .unexpectedStatus(status): "security_status_\(status)"
        case .unreadableStoredKey: "unreadable_stored_key"
        case .interactionRequired: "interaction_required"
        }
    }
}

private let keychainService = "com.liminal.pseudonym"
private let keychainAccount = "pseudonym_hmac_key"
private let keyLengthBytes = 32

/// Reads the persisted HMAC key from the Keychain, or generates and stores a new random one if
/// none exists yet. Not unit-tested against the real Keychain (CI environments can't reliably
/// guarantee an unlocked, writable login keychain) -- `hmacSha256Hex` itself, which this key
/// feeds into, is fully tested; only the "where does the key come from" storage step is not.
public func loadOrCreatePseudonymKey(allowInteraction: Bool = false) throws -> Data {
    if let existing = try readKey(allowInteraction: allowInteraction) {
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

private func readKey(allowInteraction: Bool) throws -> Data? {
    var query: [String: Any] = [
        kSecClass as String: kSecClassGenericPassword,
        kSecAttrService as String: keychainService,
        kSecAttrAccount as String: keychainAccount,
        kSecReturnData as String: true,
    ]
    // Headless capture must fail rather than block on a Keychain prompt. The visible acceptance
    // probe can opt into the normal macOS authorization path, but its caller still applies a
    // bounded timeout.
    if allowInteraction {
        // Let Security use the process's normal interactive context. Binding an LAContext from a
        // background queue can wait indefinitely even for an ordinary login-keychain item.
    } else {
        let authenticationContext = LAContext()
        authenticationContext.interactionNotAllowed = true
        query[kSecUseAuthenticationContext as String] = authenticationContext
    }
    var item: CFTypeRef?
    let status = SecItemCopyMatching(query as CFDictionary, &item)
    switch status {
    case errSecSuccess:
        guard let data = item as? Data else { throw PseudonymKeyStoreError.unreadableStoredKey }
        return data
    case errSecItemNotFound:
        return nil
    case errSecInteractionNotAllowed:
        throw PseudonymKeyStoreError.interactionRequired
    default:
        throw PseudonymKeyStoreError.unexpectedStatus(status)
    }
}

private func storeKey(_ key: Data) throws {
    let query: [String: Any] = [
        kSecClass as String: kSecClassGenericPassword,
        kSecAttrService as String: keychainService,
        kSecAttrAccount as String: keychainAccount,
        // The pseudonym must persist across daemon restarts, but it must not acquire a
        // user-presence ACL that makes a headless capture process hang. After-first-unlock is
        // the intended boundary for this local, non-exported key.
        kSecAttrAccessible as String: kSecAttrAccessibleAfterFirstUnlock,
        kSecValueData as String: key,
    ]
    let status = SecItemAdd(query as CFDictionary, nil)
    guard status == errSecSuccess else {
        throw PseudonymKeyStoreError.unexpectedStatus(status)
    }
}
