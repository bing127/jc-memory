import Foundation
import JiangchengMemoryUniffi

/// 通过 UniFFI 复用 Rust Core 的 Swift Embedded 客户端。
public final class EmbeddedMemoryClient: @unchecked Sendable {
    private let engine: JcMemoryEngine
    private var closed = false

    /// 打开本地数据库并创建一个 Swift Embedded 客户端。
    public init(dataDirectory: URL, principalId: String) throws {
        engine = try JcMemoryEngine(
            dataDir: dataDirectory.path,
            principalId: principalId
        )
    }

    /// 执行 JSON Request Envelope。
    public func execute(_ request: String) throws -> String {
        guard !closed else {
            throw EmbeddedMemoryError.closed
        }
        return try engine.execute(requestJson: request)
    }

    /// 关闭 Rust 引擎；重复关闭保持幂等。
    public func close() throws {
        guard !closed else { return }
        try engine.shutdown()
        closed = true
    }

    deinit {
        try? engine.shutdown()
    }
}

/// Swift Embedded 客户端生命周期错误。
public enum EmbeddedMemoryError: Error, Equatable, Sendable {
    /// 客户端已关闭。
    case closed
}
