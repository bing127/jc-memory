package com.jiangcheng.memory;

import org.junit.jupiter.api.Test;

import java.net.URLClassLoader;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.security.MessageDigest;
import java.util.ArrayList;
import java.util.HexFormat;

import static org.junit.jupiter.api.Assertions.*;

class NativeLibraryLoaderTest {
    @Test
    void rejectsMissingLibraryBeforeCallingNativeLoad() {
        var loads = new ArrayList<Path>();
        var loader = new NativeLibraryLoader(getClass().getClassLoader(), loads::add);

        var error = assertThrows(NativeLibraryLoadException.class, () ->
                loader.load(Path.of("/tmp/jc-memory-missing-native"), "0".repeat(64)));

        assertEquals("not_found", error.code());
        assertTrue(loads.isEmpty());
    }

    @Test
    void rejectsChecksumMismatchBeforeCallingNativeLoad() throws Exception {
        var library = Files.createTempFile("jc-memory-native", ".dylib");
        Files.writeString(library, "fixture", StandardCharsets.UTF_8);
        var loads = new ArrayList<Path>();
        var loader = new NativeLibraryLoader(getClass().getClassLoader(), loads::add);

        var error = assertThrows(NativeLibraryLoadException.class, () ->
                loader.load(library, "0".repeat(64)));

        assertEquals("checksum_mismatch", error.code());
        assertTrue(loads.isEmpty());
    }

    @Test
    void cachesSamePathForSameClassLoaderButSeparatesClassLoaders() throws Exception {
        var library = Files.createTempFile("jc-memory-native", ".dylib");
        Files.writeString(library, "fixture", StandardCharsets.UTF_8);
        var digest = sha256(library);
        var loads = new ArrayList<Path>();
        var first = new NativeLibraryLoader(new URLClassLoader(new java.net.URL[0], null), loads::add);
        var second = new NativeLibraryLoader(new URLClassLoader(new java.net.URL[0], null), loads::add);

        first.load(library, digest);
        first.load(library, digest.toUpperCase());
        second.load(library, digest);

        assertEquals(2, loads.size());
    }

    @Test
    void rejectsDifferentExpectedDigestAfterTheLibraryIsCached() throws Exception {
        var library = Files.createTempFile("jc-memory-native", ".dylib");
        Files.writeString(library, "fixture", StandardCharsets.UTF_8);
        var digest = sha256(library);
        var loads = new ArrayList<Path>();
        var loader = new NativeLibraryLoader(getClass().getClassLoader(), loads::add);
        loader.load(library, digest);

        var error = assertThrows(NativeLibraryLoadException.class, () ->
                loader.load(library, "0".repeat(64)));

        assertEquals("checksum_mismatch", error.code());
        assertEquals(1, loads.size());
    }

    @Test
    void mapsNativeLinkageFailureToStableLoadError() throws Exception {
        var library = Files.createTempFile("jc-memory-native", ".dylib");
        Files.writeString(library, "fixture", StandardCharsets.UTF_8);
        var digest = sha256(library);
        var loader = new NativeLibraryLoader(
                getClass().getClassLoader(),
                ignored -> { throw new UnsatisfiedLinkError("bad native library"); });

        var error = assertThrows(NativeLibraryLoadException.class, () -> loader.load(library, digest));

        assertEquals("load_failed", error.code());
        assertInstanceOf(UnsatisfiedLinkError.class, error.getCause());
    }

    private static String sha256(Path path) throws Exception {
        return HexFormat.of().formatHex(MessageDigest.getInstance("SHA-256").digest(Files.readAllBytes(path)));
    }
}
