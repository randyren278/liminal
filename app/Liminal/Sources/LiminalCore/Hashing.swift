import CryptoKit
import Foundation
import IOKit

/// SHA-256 hex digest, prefixed. Used everywhere a hardware identifier must be hashed before
/// leaving the device -- master plan §22 (`device_id_hash`) and the same privacy posture as
/// `liminal_policy::pseudonymize` on the Rust side (though this uses a plain hash, not HMAC,
/// since there is no shared local key at this layer yet -- these are machine/device identity
/// hashes, not sensor-reading pseudonyms subject to §36/§39's HMAC requirement).
public func sha256Hex(_ input: String, prefix: String) -> String {
    let digest = SHA256.hash(data: Data(input.utf8))
    let hex = digest.map { String(format: "%02x", $0) }.joined()
    return "\(prefix):\(hex)"
}

/// HMAC-SHA256(key, message), hex-encoded, prefixed. Mirrors `liminal_policy::pseudonymize` on
/// the Rust side exactly (same algorithm, same `prefix:hex` shape) -- master plan §36/§39: a
/// sensor-reading identifier (BLE peripheral UUID, a future Wi-Fi AP identifier under Mode B)
/// must be transformed through HMAC with a locally-held key before it ever leaves this function,
/// never a plain hash (unlike `sha256Hex` above, which is for one-way machine/device identity
/// where no shared key or cross-session stability requirement exists).
public func hmacSha256Hex(key: Data, message: String, prefix: String) -> String {
    let key = SymmetricKey(data: key)
    let mac = HMAC<SHA256>.authenticationCode(for: Data(message.utf8), using: key)
    let hex = mac.map { String(format: "%02x", $0) }.joined()
    return "\(prefix):\(hex)"
}

/// A stable-per-machine, non-reversible identifier for this Mac. Reads `IOPlatformUUID` (a
/// hardware-derived UUID Apple exposes for exactly this purpose) and hashes it immediately --
/// the raw UUID is never persisted or logged.
public func machineProfileId() -> String {
    let platformExpert = IOServiceGetMatchingService(
        kIOMainPortDefault, IOServiceMatching("IOPlatformExpertDevice"),
    )
    defer { IOObjectRelease(platformExpert) }

    guard platformExpert != 0,
          let uuidRef = IORegistryEntryCreateCFProperty(
              platformExpert, "IOPlatformUUID" as CFString, kCFAllocatorDefault, 0,
          )
    else {
        return sha256Hex("unknown-platform-uuid", prefix: "machine")
    }
    let uuid = (uuidRef.takeRetainedValue() as? String) ?? "unknown-platform-uuid"
    return sha256Hex(uuid, prefix: "machine")
}
