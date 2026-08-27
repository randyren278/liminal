import Darwin
import Foundation

/// A minimal Unix domain socket client -- master plan §15 (Unix domain sockets under
/// `/tmp/liminal-$UID/`). Foundation has no built-in Unix-socket client API on macOS, so this
/// wraps the POSIX socket calls directly.
public enum UnixSocketError: Error, Equatable {
    case pathTooLong
    case socketCreationFailed(errno: Int32)
    case connectFailed(errno: Int32)
    case writeFailed(errno: Int32)
}

public final class UnixSocketClient {
    private let fd: Int32

    /// Connects to the Unix domain socket at `path`. Throws immediately if the socket doesn't
    /// exist or nothing is listening -- callers decide how to handle that (e.g. `liminal-capture`
    /// falls back to printing to stdout when `liminald` isn't running yet).
    public init(path: String) throws {
        let created = socket(AF_UNIX, SOCK_STREAM, 0)
        guard created >= 0 else {
            throw UnixSocketError.socketCreationFailed(errno: errno)
        }
        fd = created

        var noSigPipe: Int32 = 1
        guard setsockopt(
            created,
            SOL_SOCKET,
            SO_NOSIGPIPE,
            &noSigPipe,
            socklen_t(MemoryLayout<Int32>.size),
        ) == 0 else {
            let savedErrno = errno
            close(created)
            throw UnixSocketError.socketCreationFailed(errno: savedErrno)
        }

        var addr = sockaddr_un()
        addr.sun_family = sa_family_t(AF_UNIX)
        let pathBytes = Array(path.utf8)
        guard pathBytes.count < MemoryLayout.size(ofValue: addr.sun_path) else {
            close(created)
            throw UnixSocketError.pathTooLong
        }
        withUnsafeMutableBytes(of: &addr.sun_path) { rawPtr in
            let buffer = rawPtr.bindMemory(to: CChar.self)
            for (i, byte) in pathBytes.enumerated() {
                buffer[i] = CChar(bitPattern: byte)
            }
            buffer[pathBytes.count] = 0
        }

        let result = withUnsafePointer(to: &addr) { ptr -> Int32 in
            ptr.withMemoryRebound(to: sockaddr.self, capacity: 1) { sockaddrPtr in
                connect(created, sockaddrPtr, socklen_t(MemoryLayout<sockaddr_un>.size))
            }
        }
        guard result == 0 else {
            let savedErrno = errno
            close(created)
            throw UnixSocketError.connectFailed(errno: savedErrno)
        }
    }

    /// Writes the full contents of `data`, looping over partial writes.
    public func write(_ data: Data) throws {
        try data.withUnsafeBytes { (rawPtr: UnsafeRawBufferPointer) in
            var offset = 0
            while offset < rawPtr.count {
                let n = Darwin.write(fd, rawPtr.baseAddress!.advanced(by: offset), rawPtr.count - offset)
                if n < 0 {
                    throw UnixSocketError.writeFailed(errno: errno)
                }
                guard n > 0 else {
                    throw UnixSocketError.writeFailed(errno: 0)
                }
                offset += n
            }
        }
    }

    deinit {
        close(fd)
    }
}
