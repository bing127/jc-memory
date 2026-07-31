import Foundation

/// Swift SDK 的稳定远程错误。
public struct JiangchengMemoryError: Error, Equatable, Sendable {
    public let code: String
    public let requestId: String?
    public let details: String?
    public let status: Int?
    public let message: String

    public init(code: String, message: String, requestId: String? = nil, details: String? = nil, status: Int? = nil) {
        self.code = code
        self.message = message
        self.requestId = requestId
        self.details = details
        self.status = status
    }
}

/// 统一 JSON Response Envelope 的轻量包装。
public struct MemoryResponse: Equatable, Sendable {
    public let json: Data

    public init(json: Data) {
        self.json = json
    }
}

/// 使用 actor 保护 URLSession 和关闭状态的异步客户端。
public actor JiangchengMemoryClient {
    private let baseURL: URL
    private let apiKey: String?
    private let session: URLSession
    private var closed = false

    public init(baseURL: URL, apiKey: String? = nil, session: URLSession = .shared) {
        self.baseURL = baseURL
        self.apiKey = apiKey
        self.session = session
    }

    /// 写入一个原始 Fragment。
    public func observe(_ request: Data) async throws -> MemoryResponse {
        try await execute(path: "/v1/memory/observe", request: request)
    }

    /// 提交一次完整状态转移的 Fragment 集合。
    public func commit(_ request: Data) async throws -> MemoryResponse {
        try await execute(path: "/v1/memory/commit", request: request)
    }

    /// 执行有界 Recall。
    public func recall(_ request: Data) async throws -> MemoryResponse {
        try await execute(path: "/v1/memory/recall", request: request)
    }

    /// 读取 Scope 统计。
    public func stats(_ request: Data) async throws -> MemoryResponse {
        try await execute(path: "/v1/memory/stats", request: request)
    }

    /// 追加纠正记录。
    public func correct(_ request: Data) async throws -> MemoryResponse {
        try await execute(path: "/v1/memory/correct", request: request)
    }

    /// 追加失效记录。
    public func forget(_ request: Data) async throws -> MemoryResponse {
        try await execute(path: "/v1/memory/forget", request: request)
    }

    /// 返回 Recall 的结构化证据追踪。
    public func explain(_ request: Data) async throws -> MemoryResponse {
        try await execute(path: "/v1/memory/explain", request: request)
    }

    /// 关闭客户端；URLSession 由系统管理，重复关闭保持幂等。
    public func close() {
        closed = true
    }

    private func execute(path: String, request: Data) async throws -> MemoryResponse {
        guard !closed else {
            throw JiangchengMemoryError(code: "internal", message: "客户端已经关闭")
        }
        try Task.checkCancellation()
        var urlRequest = URLRequest(url: baseURL.appendingPathComponent(String(path.dropFirst())))
        urlRequest.httpMethod = "POST"
        urlRequest.httpBody = request
        urlRequest.setValue("application/json", forHTTPHeaderField: "content-type")
        urlRequest.setValue("application/json", forHTTPHeaderField: "accept")
        if let apiKey, !apiKey.isEmpty {
            urlRequest.setValue("Bearer \(apiKey)", forHTTPHeaderField: "authorization")
        }
        let (data, response): (Data, URLResponse)
        do {
            (data, response) = try await session.data(for: urlRequest)
        } catch is CancellationError {
            throw CancellationError()
        } catch {
            throw JiangchengMemoryError(code: "transport", message: "无法连接疆诚记忆服务: \(error.localizedDescription)")
        }
        guard let httpResponse = response as? HTTPURLResponse else {
            throw JiangchengMemoryError(code: "internal", message: "服务没有返回 HTTP 响应")
        }
        guard (200..<300).contains(httpResponse.statusCode) else {
            throw parseError(data: data, status: httpResponse.statusCode)
        }
        return MemoryResponse(json: data)
    }

    private func parseError(data: Data, status: Int) -> JiangchengMemoryError {
        let value = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any]
        let code = (value?["code"] as? String) ?? codeForStatus(status)
        let requestId = value?["request_id"] as? String
        let message = (value?["message"] as? String) ?? "HTTP \(status)"
        return JiangchengMemoryError(code: code, message: message, requestId: requestId, details: String(data: data, encoding: .utf8), status: status)
    }

    private func codeForStatus(_ status: Int) -> String {
        switch status {
        case 401: return "unauthorized"
        case 403: return "permission_denied"
        case 404: return "not_found"
        case 408, 504: return "deadline_exceeded"
        case 409: return "conflict"
        case 429: return "resource_exhausted"
        case 400..<500: return "invalid_argument"
        default: return "internal"
        }
    }
}
