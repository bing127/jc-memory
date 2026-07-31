import Foundation
import Testing
@testable import JiangchengMemory

@Test("UniFFI 生成 binding 保留 Embedded API")
func generatedUniFFIBindingContractIsPresent() throws {
    let sourcePath = URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent()
        .deletingLastPathComponent()
        .deletingLastPathComponent()
        .appendingPathComponent("Sources/JiangchengMemoryUniffi/jc_memory_uniffi.swift")
    let source = try String(contentsOf: sourcePath, encoding: .utf8)
    #expect(source.contains("public protocol JcMemoryEngineProtocol"))
    #expect(source.contains("func execute(requestJson: String)"))
    #expect(source.contains("func shutdown()"))
}

@Test("返回 canonical Recall response 并携带 Bearer API Key")
func recallReturnsFixture() async throws {
    let fixture = try loadFixture()
    let request = try jsonData(fixture["request"] as Any)
    let expected = try jsonData(fixture["response"] as Any)
    FixtureURLProtocol.responseBody = expected
    let configuration = URLSessionConfiguration.ephemeral
    configuration.protocolClasses = [FixtureURLProtocol.self]
    let session = URLSession(configuration: configuration)
    let client = JiangchengMemoryClient(baseURL: URL(string: "https://fixture.invalid")!, apiKey: "fixture", session: session)
    let response = try await client.recall(request)
    #expect(response.json == expected)
    #expect(FixtureURLProtocol.authorization == "Bearer fixture")
}

@Test("关闭后拒绝新的命令")
func closeRejectsCalls() async throws {
    let client = JiangchengMemoryClient(baseURL: URL(string: "https://fixture.invalid")!)
    await client.close()
    await #expect(throws: JiangchengMemoryError.self) {
        _ = try await client.recall(Data("{}".utf8))
    }
}

@Test("调用方取消会保留 CancellationError")
func cancellationPropagates() async throws {
    let client = JiangchengMemoryClient(baseURL: URL(string: "https://fixture.invalid")!)
    let task = Task {
        try await client.recall(Data("{}".utf8))
    }
    task.cancel()
    await #expect(throws: CancellationError.self) {
        _ = try await task.value
    }
}

private func loadFixture() throws -> [String: Any] {
    let path = URL(fileURLWithPath: "../contracts/fixtures/recall-exact.json")
    let data = try Data(contentsOf: path)
    return try #require(JSONSerialization.jsonObject(with: data) as? [String: Any])
}

private func jsonData(_ value: Any) throws -> Data {
    try JSONSerialization.data(withJSONObject: value, options: [.sortedKeys])
}

private final class FixtureURLProtocol: URLProtocol {
    // 测试只串行使用 URLProtocol；生产客户端不持有可变全局状态。
    nonisolated(unsafe) static var responseBody = Data()
    nonisolated(unsafe) static var authorization: String?

    override class func canInit(with request: URLRequest) -> Bool { true }

    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }

    override func startLoading() {
        Self.authorization = request.value(forHTTPHeaderField: "authorization")
        let response = HTTPURLResponse(
            url: request.url!,
            statusCode: 200,
            httpVersion: "HTTP/1.1",
            headerFields: ["content-type": "application/json"]
        )!
        client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: Self.responseBody)
        client?.urlProtocolDidFinishLoading(self)
    }

    override func stopLoading() {}
}
