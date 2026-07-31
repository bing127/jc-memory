package com.jiangcheng.memory

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.flow
import kotlinx.coroutines.future.await
import kotlinx.coroutines.withContext
import java.net.URI
import java.net.http.HttpClient
import java.net.http.HttpRequest
import java.net.http.HttpResponse

/** 远程服务返回的稳定错误。 */
class JiangchengMemoryError(
    val code: String,
    val requestId: String? = null,
    val details: String? = null,
    val status: Int? = null,
    message: String,
) : RuntimeException(message)

/** 统一 JSON Response Envelope 的轻量包装。 */
data class MemoryResponse(val json: String)

/** 使用 JDK HttpClient 和 Kotlin Coroutine 访问疆诚记忆服务。 */
class JiangchengMemoryClient(
    baseUrl: String,
    private val apiKey: String? = null,
    private val httpClient: HttpClient = HttpClient.newBuilder().build(),
) : AutoCloseable {
    private val baseUrl = baseUrl.trimEnd('/')
    private var closed = false

    /** 写入一个原始 Fragment。 */
    suspend fun observe(requestJson: String): MemoryResponse = execute("/v1/memory/observe", requestJson)

    /** 提交一次完整状态转移的 Fragment 集合。 */
    suspend fun commit(requestJson: String): MemoryResponse = execute("/v1/memory/commit", requestJson)

    /** 执行有界 Recall。 */
    suspend fun recall(requestJson: String): MemoryResponse = execute("/v1/memory/recall", requestJson)

    /** 读取 Scope 统计。 */
    suspend fun stats(requestJson: String): MemoryResponse = execute("/v1/memory/stats", requestJson)

    /** 追加纠正记录。 */
    suspend fun correct(requestJson: String): MemoryResponse = execute("/v1/memory/correct", requestJson)

    /** 追加失效记录。 */
    suspend fun forget(requestJson: String): MemoryResponse = execute("/v1/memory/forget", requestJson)

    /** 返回 Recall 的结构化证据追踪。 */
    suspend fun explain(requestJson: String): MemoryResponse = execute("/v1/memory/explain", requestJson)

    /** 将一次 Recall 暴露为可取消的单元素 Flow。 */
    fun recallFlow(requestJson: String): Flow<MemoryResponse> = flow {
        emit(recall(requestJson))
    }

    override fun close() {
        closed = true
    }

    private suspend fun execute(path: String, requestJson: String): MemoryResponse = withContext(Dispatchers.IO) {
        if (closed) {
            throw JiangchengMemoryError("internal", message = "客户端已经关闭")
        }
        if (baseUrl.isBlank()) {
            throw JiangchengMemoryError("invalid_argument", message = "baseUrl 不能为空")
        }
        val builder = HttpRequest.newBuilder()
            .uri(URI.create(baseUrl + path))
            .header("content-type", "application/json")
            .header("accept", "application/json")
            .POST(HttpRequest.BodyPublishers.ofString(requestJson))
        apiKey?.takeIf { it.isNotBlank() }?.let { builder.header("authorization", "Bearer $it") }
        val response = try {
            httpClient.sendAsync(builder.build(), HttpResponse.BodyHandlers.ofString()).await()
        } catch (cancelled: kotlinx.coroutines.CancellationException) {
            throw cancelled
        } catch (error: Exception) {
            throw JiangchengMemoryError("transport", message = "无法连接疆诚记忆服务: ${error.message}")
        }
        if (response.statusCode() !in 200..299) {
            throw parseError(response)
        }
        MemoryResponse(response.body())
    }

    private fun parseError(response: HttpResponse<String>): JiangchengMemoryError {
        val body = response.body()
        val code = jsonField(body, "code") ?: codeForStatus(response.statusCode())
        val requestId = jsonField(body, "request_id")
        val message = jsonField(body, "message") ?: "HTTP ${response.statusCode()}"
        return JiangchengMemoryError(code, requestId, body, response.statusCode(), message)
    }

    private fun codeForStatus(status: Int): String = when (status) {
        401 -> "unauthorized"
        403 -> "permission_denied"
        404 -> "not_found"
        408, 504 -> "deadline_exceeded"
        409 -> "conflict"
        429 -> "resource_exhausted"
        in 400..499 -> "invalid_argument"
        else -> "internal"
    }

    private fun jsonField(body: String, field: String): String? =
        Regex("\\\"${Regex.escape(field)}\\\"\\s*:\\s*\\\"([^\\\"]*)\\\"").find(body)?.groupValues?.get(1)
}
