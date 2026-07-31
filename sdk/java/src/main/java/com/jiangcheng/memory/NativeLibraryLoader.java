package com.jiangcheng.memory;

import java.io.IOException;
import java.io.InputStream;
import java.nio.file.FileAlreadyExistsException;
import java.nio.file.Files;
import java.nio.file.LinkOption;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.nio.file.attribute.BasicFileAttributes;
import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;
import java.util.HexFormat;
import java.util.Map;
import java.util.Objects;
import java.util.WeakHashMap;

/** 在 JVM 加载 native library 前校验摘要并按 classloader 缓存。 */
public final class NativeLibraryLoader {
    /** 实际执行 native load 的边界，测试可以注入不触发系统加载的实现。 */
    @FunctionalInterface
    public interface NativeLibrarySystem {
        void load(Path path);
    }

    /** 已完成校验和加载的 native library。 */
    public record LoadedNativeLibrary(Path path, String sha256) {
    }

    private final ClassLoader classLoader;
    private final NativeLibrarySystem nativeLibrarySystem;
    private final Map<ClassLoader, Map<Path, LoadedNativeLibrary>> loaded = new WeakHashMap<>();

    public NativeLibraryLoader() {
        this(NativeLibraryLoader.class.getClassLoader(), path -> System.load(path.toString()));
    }

    NativeLibraryLoader(ClassLoader classLoader, NativeLibrarySystem nativeLibrarySystem) {
        this.classLoader = classLoader;
        this.nativeLibrarySystem = Objects.requireNonNull(nativeLibrarySystem, "nativeLibrarySystem");
    }

    /** 校验并加载一个明确路径的 native library。 */
    public LoadedNativeLibrary load(Path library, String expectedSha256) {
        String expected = normalizeExpected(expectedSha256, library);
        Path absolute = library.toAbsolutePath().normalize();
        ensureRegularFile(absolute, expected);
        String actual = digest(absolute, expected);
        if (!expected.equals(actual)) {
            throw checksumMismatch(absolute, expected, actual);
        }
        synchronized (loaded) {
            Map<Path, LoadedNativeLibrary> classLoaderCache =
                    loaded.computeIfAbsent(classLoader, ignored -> new java.util.HashMap<>());
            LoadedNativeLibrary cached = classLoaderCache.get(absolute);
            if (cached != null) {
                if (!cached.sha256().equals(expected)) {
                    throw checksumMismatch(absolute, expected, cached.sha256());
                }
                return cached;
            }
            try {
                nativeLibrarySystem.load(absolute);
            } catch (RuntimeException | LinkageError error) {
                throw new NativeLibraryLoadException(
                        "load_failed",
                        "native library 加载失败: " + absolute,
                        absolute,
                        expected,
                        actual,
                        error);
            }
            LoadedNativeLibrary loadedLibrary = new LoadedNativeLibrary(absolute, actual);
            classLoaderCache.put(absolute, loadedLibrary);
            return loadedLibrary;
        }
    }

    /** 从 classpath resource 原子复制、校验并加载 native library。 */
    public LoadedNativeLibrary loadResource(
            String resourceName,
            Path destinationDirectory,
            String expectedSha256) {
        Objects.requireNonNull(resourceName, "resourceName");
        Objects.requireNonNull(destinationDirectory, "destinationDirectory");
        String expected = normalizeExpected(expectedSha256, destinationDirectory);
        Path destination = destinationDirectory
                .toAbsolutePath()
                .normalize()
                .resolve(Path.of(resourceName).getFileName().toString())
                .normalize();
        if (Files.exists(destination, LinkOption.NOFOLLOW_LINKS)) {
            return load(destination, expected);
        }
        try {
            Files.createDirectories(destination.getParent());
            ClassLoader resources = classLoader == null
                    ? ClassLoader.getSystemClassLoader()
                    : classLoader;
            try (InputStream input = resources.getResourceAsStream(resourceName)) {
                if (input == null) {
                    throw new NativeLibraryLoadException(
                            "not_found",
                            "classpath resource 不存在: " + resourceName,
                            destination,
                            expected,
                            null,
                            null);
                }
                Path temporary = Files.createTempFile(destination.getParent(), ".jcmemory-", ".tmp");
                try {
                    Files.copy(input, temporary, StandardCopyOption.REPLACE_EXISTING);
                    String actual = digest(temporary, expected);
                    if (!expected.equals(actual)) {
                        throw checksumMismatch(temporary, expected, actual);
                    }
                    try {
                        Files.move(temporary, destination, StandardCopyOption.ATOMIC_MOVE);
                    } catch (FileAlreadyExistsException ignored) {
                        // 并发调用已经发布了同一个目标，下面重新校验目标内容。
                    }
                } finally {
                    Files.deleteIfExists(temporary);
                }
            }
            return load(destination, expected);
        } catch (NativeLibraryLoadException error) {
            throw error;
        } catch (IOException error) {
            throw new NativeLibraryLoadException(
                    "io_error",
                    "native resource 发布失败: " + resourceName,
                    destination,
                    expected,
                    null,
                    error);
        }
    }

    private static void ensureRegularFile(Path path, String expected) {
        try {
            BasicFileAttributes attributes = Files.readAttributes(
                    path,
                    BasicFileAttributes.class,
                    LinkOption.NOFOLLOW_LINKS);
            if (!attributes.isRegularFile()) {
                throw new NativeLibraryLoadException(
                        "invalid_argument",
                        "native library 不是普通文件: " + path,
                        path,
                        expected,
                        null,
                        null);
            }
        } catch (IOException error) {
            throw new NativeLibraryLoadException(
                    "not_found",
                    "native library 不存在或不可读取: " + path,
                    path,
                    expected,
                    null,
                    error);
        }
    }

    private static String digest(Path path, String expected) {
        try {
            MessageDigest digest = MessageDigest.getInstance("SHA-256");
            try (InputStream input = Files.newInputStream(path)) {
                byte[] buffer = new byte[8192];
                int read;
                while ((read = input.read(buffer)) >= 0) {
                    if (read > 0) {
                        digest.update(buffer, 0, read);
                    }
                }
            }
            return HexFormat.of().formatHex(digest.digest());
        } catch (NoSuchAlgorithmException | IOException error) {
            throw new NativeLibraryLoadException(
                    "io_error",
                    "native library 摘要计算失败: " + path,
                    path,
                    expected,
                    null,
                    error);
        }
    }

    private static String normalizeExpected(String expected, Path path) {
        if (expected == null || !expected.matches("[0-9a-fA-F]{64}")) {
            throw new NativeLibraryLoadException(
                    "invalid_argument",
                    "expected SHA-256 必须是 64 位十六进制字符串",
                    path,
                    expected,
                    null,
                    null);
        }
        return expected.toLowerCase(java.util.Locale.ROOT);
    }

    private static NativeLibraryLoadException checksumMismatch(Path path, String expected, String actual) {
        return new NativeLibraryLoadException(
                "checksum_mismatch",
                "native library SHA-256 校验失败: " + path,
                path,
                expected,
                actual,
                null);
    }
}
