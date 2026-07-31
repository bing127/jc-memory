package com.jiangcheng.memory;

import com.sun.net.httpserver.HttpServer;
import org.junit.jupiter.api.Test;

import java.net.InetSocketAddress;
import java.net.http.HttpClient;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.time.Duration;
import java.util.concurrent.CancellationException;
import java.util.concurrent.CompletionException;

import static org.junit.jupiter.api.Assertions.*;

class JiangchengMemoryClientTest {
    @Test
    void returnsCanonicalRecallFixtureAndBearerKey() throws Exception {
        var fixture = fixture("recall-exact.json");
        var request = field(fixture, "request");
        var responseBody = field(fixture, "response");
        var authorization = new String[1];
        var server = HttpServer.create(new InetSocketAddress("127.0.0.1", 0), 0);
        server.createContext("/v1/memory/recall", exchange -> {
            authorization[0] = exchange.getRequestHeaders().getFirst("Authorization");
            exchange.getResponseHeaders().add("content-type", "application/json");
            byte[] data = responseBody.getBytes(StandardCharsets.UTF_8);
            exchange.sendResponseHeaders(200, data.length);
            try (var output = exchange.getResponseBody()) {
                output.write(data);
            }
        });
        server.start();
        try {
            var client = new JiangchengMemoryClient(
                    "http://127.0.0.1:" + server.getAddress().getPort(),
                    "fixture",
                    HttpClient.newHttpClient());
            var response = client.recall(request).join();
            assertEquals(responseBody, response.json());
            assertEquals("Bearer fixture", authorization[0]);
            client.close();
        } finally {
            server.stop(0);
        }
    }

    @Test
    void mapsHttpErrorAndRequestId() throws Exception {
        var server = HttpServer.create(new InetSocketAddress("127.0.0.1", 0), 0);
        server.createContext("/v1/memory/recall", exchange -> {
            byte[] data = "{\"code\":\"invalid_argument\",\"message\":\"bad\",\"request_id\":\"fixture\"}"
                    .getBytes(StandardCharsets.UTF_8);
            exchange.sendResponseHeaders(400, data.length);
            try (var output = exchange.getResponseBody()) {
                output.write(data);
            }
        });
        server.start();
        try {
            var client = new JiangchengMemoryClient("http://127.0.0.1:" + server.getAddress().getPort());
            var error = assertThrows(CompletionException.class, () -> client.recall("{}").join());
            assertInstanceOf(JiangchengMemoryError.class, error.getCause());
            var memoryError = (JiangchengMemoryError) error.getCause();
            assertEquals("invalid_argument", memoryError.code());
            assertEquals("fixture", memoryError.requestId());
        } finally {
            server.stop(0);
        }
    }

    @Test
    void cancellationIsVisibleToCompletableFuture() {
        var client = new JiangchengMemoryClient("http://127.0.0.1:1", null, HttpClient.newHttpClient());
        var future = client.recall("{}");
        assertTrue(future.cancel(true) || future.isCancelled());
        assertThrows(CancellationException.class, future::join);
    }

    @Test
    void blockingFacadeReturnsTheSameFixtureResponse() throws Exception {
        var fixture = fixture("recall-exact.json");
        var request = field(fixture, "request");
        var responseBody = field(fixture, "response");
        var server = HttpServer.create(new InetSocketAddress("127.0.0.1", 0), 0);
        server.createContext("/v1/memory/recall", exchange -> {
            byte[] data = responseBody.getBytes(StandardCharsets.UTF_8);
            exchange.sendResponseHeaders(200, data.length);
            try (var output = exchange.getResponseBody()) {
                output.write(data);
            }
        });
        server.start();
        try {
            var client = new JiangchengMemoryClient("http://127.0.0.1:" + server.getAddress().getPort());
            assertEquals(responseBody, client.recallBlocking(request).json());
        } finally {
            server.stop(0);
        }
    }

    private static String fixture(String name) throws Exception {
        return Files.readString(Path.of("../contracts/fixtures", name));
    }

    private static String field(String json, String field) {
        String marker = "\"" + field + "\":";
        int start = json.indexOf(marker);
        if (start < 0) throw new AssertionError("缺少字段 " + field);
        start += marker.length();
        int depth = 0;
        boolean quoted = false;
        for (int index = start; index < json.length(); index++) {
            char current = json.charAt(index);
            if (current == '"' && (index == 0 || json.charAt(index - 1) != '\\')) quoted = !quoted;
            if (quoted) continue;
            if (current == '{') depth++;
            if (current == '}') {
                depth--;
                if (depth == 0) return json.substring(start, index + 1);
            }
        }
        throw new AssertionError("字段不是 JSON 对象: " + field);
    }
}
