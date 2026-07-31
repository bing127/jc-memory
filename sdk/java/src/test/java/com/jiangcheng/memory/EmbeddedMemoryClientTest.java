package com.jiangcheng.memory;

import org.junit.jupiter.api.Test;

import java.nio.file.Files;
import java.nio.file.Path;
import java.security.MessageDigest;
import java.util.HexFormat;

import static org.junit.jupiter.api.Assertions.*;
import static org.junit.jupiter.api.Assumptions.assumeTrue;

class EmbeddedMemoryClientTest {
    @Test
    void executesRecallFixtureThroughCurrentHostNativeLibrary() throws Exception {
        Path library = Path.of("../../target/debug/libjc_memory_uniffi.dylib").toAbsolutePath().normalize();
        assumeTrue(Files.isRegularFile(library), "先运行 scripts/build/build-java-uniffi.sh 构建 native library");
        var fixture = Files.readString(Path.of("../contracts/fixtures/recall-exact.json"));
        var request = field(fixture, "request");
        var dataDirectory = Files.createTempDirectory("jc-memory-java-embedded");

        try (var client = EmbeddedMemoryClient.open(
                library,
                sha256(library),
                dataDirectory,
                "fixture-principal")) {
            var response = client.recall(request).json();
            assertTrue(response.contains("\"request_id\":\"fixture-recall-exact\""));
            assertTrue(response.contains("\"response\":\"recall\""));
        }
    }

    @Test
    void rejectsChecksumBeforeOpeningEmbeddedHandle() throws Exception {
        Path library = Path.of("../../target/debug/libjc_memory_uniffi.dylib").toAbsolutePath().normalize();
        assumeTrue(Files.isRegularFile(library), "先运行 scripts/build/build-java-uniffi.sh 构建 native library");

        var error = assertThrows(NativeLibraryLoadException.class, () -> EmbeddedMemoryClient.open(
                library,
                "0".repeat(64),
                Files.createTempDirectory("jc-memory-java-embedded"),
                "fixture-principal"));

        assertEquals("checksum_mismatch", error.code());
    }

    @Test
    void closeIsIdempotentAndRejectsCallsAfterShutdown() throws Exception {
        Path library = Path.of("../../target/debug/libjc_memory_uniffi.dylib").toAbsolutePath().normalize();
        assumeTrue(Files.isRegularFile(library), "先运行 scripts/build/build-java-uniffi.sh 构建 native library");
        var client = EmbeddedMemoryClient.open(
                library,
                sha256(library),
                Files.createTempDirectory("jc-memory-java-embedded"),
                "fixture-principal");

        client.close();
        client.close();
        var error = assertThrows(JiangchengMemoryError.class, () -> client.recall("{}"));
        assertEquals("internal", error.code());
    }

    private static String sha256(Path path) throws Exception {
        return HexFormat.of().formatHex(MessageDigest.getInstance("SHA-256").digest(Files.readAllBytes(path)));
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
