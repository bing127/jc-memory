package com.jiangcheng.memory

import com.sun.net.httpserver.HttpServer
import kotlinx.coroutines.async
import kotlinx.coroutines.cancelAndJoin
import kotlinx.coroutines.delay
import kotlinx.coroutines.runBlocking
import java.net.InetSocketAddress
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith

class JiangchengMemoryClientTest {
    @Test
    fun `recall returns canonical fixture and carries bearer key`() = runBlocking {
        val fixture = fixture("recall-exact.json")
        var authorization = ""
        val server = HttpServer.create(InetSocketAddress("127.0.0.1", 0), 0)
        server.createContext("/v1/memory/recall") { exchange ->
            authorization = exchange.requestHeaders.getFirst("Authorization")
            exchange.responseHeaders.add("content-type", "application/json")
            val response = fixtureResponse(fixture)
            exchange.sendResponseHeaders(200, response.toByteArray().size.toLong())
            exchange.responseBody.use { it.write(response.toByteArray()) }
        }
        server.start()
        try {
            val client = JiangchengMemoryClient("http://127.0.0.1:${server.address.port}", "fixture")
            assertEquals(fixtureResponse(fixture), client.recall(fixtureRequest(fixture)).json)
            assertEquals("Bearer fixture", authorization)
            client.close()
        } finally {
            server.stop(0)
        }
    }

    @Test
    fun `cancellation cancels in flight request`() = runBlocking {
        val server = HttpServer.create(InetSocketAddress("127.0.0.1", 0), 0)
        server.createContext("/v1/memory/recall") {
            Thread.sleep(5_000)
        }
        server.start()
        try {
            val client = JiangchengMemoryClient("http://127.0.0.1:${server.address.port}")
            val pending = async { client.recall("{}") }
            delay(100)
            pending.cancelAndJoin()
        } finally {
            server.stop(0)
        }
    }

    @Test
    fun `maps HTTP errors to stable exception`() = runBlocking {
        val server = HttpServer.create(InetSocketAddress("127.0.0.1", 0), 0)
        server.createContext("/v1/memory/recall") { exchange ->
            val response = "{\"code\":\"invalid_argument\",\"message\":\"bad\",\"request_id\":\"fixture\"}"
            exchange.sendResponseHeaders(400, response.toByteArray().size.toLong())
            exchange.responseBody.use { it.write(response.toByteArray()) }
        }
        server.start()
        try {
            val client = JiangchengMemoryClient("http://127.0.0.1:${server.address.port}")
            val error = assertFailsWith<JiangchengMemoryError> { client.recall("{}") }
            assertEquals("invalid_argument", error.code)
            assertEquals("fixture", error.requestId)
        } finally {
            server.stop(0)
        }
    }

    private fun fixture(name: String): String =
        java.io.File("../contracts/fixtures/$name").readText()

    private fun fixtureRequest(fixture: String): String =
        Regex("\\\"request\\\":(\\{.*?\\}),\\\"response\\\":", RegexOption.DOT_MATCHES_ALL)
            .find(fixture)?.groupValues?.get(1) ?: error("缺少 request Fixture")

    private fun fixtureResponse(fixture: String): String =
        Regex("\\\"response\\\":(\\{.*\\}),\\\"canonical_sha256\\\"", RegexOption.DOT_MATCHES_ALL)
            .find(fixture)?.groupValues?.get(1) ?: error("缺少 response Fixture")
}
