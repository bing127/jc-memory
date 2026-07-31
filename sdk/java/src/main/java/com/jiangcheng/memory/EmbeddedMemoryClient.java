package com.jiangcheng.memory;

import uniffi.jc_memory_uniffi.JcMemoryEngine;
import uniffi.jc_memory_uniffi.JcMemoryException;

import java.nio.file.Path;
import java.util.Objects;

/** 通过 UniFFI/JVM binding 复用 Rust Core 的同步 Embedded 客户端。 */
public final class EmbeddedMemoryClient implements AutoCloseable {
    private static final String COMPONENT = "jc_memory_uniffi";
    private static final String LIBRARY_OVERRIDE = "uniffi.component." + COMPONENT + ".libraryOverride";
    private static final Object LIBRARY_CONFIGURATION_LOCK = new Object();
    private static String configuredLibrary;

    private final JcMemoryEngine engine;
    private boolean closed;

    /** 校验并加载 native library 后创建 Embedded 客户端。 */
    public static EmbeddedMemoryClient open(
            Path library,
            String expectedSha256,
            Path dataDirectory,
            String principalId) {
        return new EmbeddedMemoryClient(
                new NativeLibraryLoader(),
                library,
                expectedSha256,
                dataDirectory,
                principalId);
    }

    EmbeddedMemoryClient(
            NativeLibraryLoader loader,
            Path library,
            String expectedSha256,
            Path dataDirectory,
            String principalId) {
        Objects.requireNonNull(loader, "loader");
        Objects.requireNonNull(library, "library");
        Objects.requireNonNull(dataDirectory, "dataDirectory");
        Objects.requireNonNull(principalId, "principalId");
        NativeLibraryLoader.LoadedNativeLibrary loaded = loader.load(library, expectedSha256);
        configureLibrary(loaded.path());
        this.engine = new JcMemoryEngine(dataDirectory.toString(), principalId);
    }

    /** 执行任意 JSON Request Envelope。 */
    public synchronized MemoryResponse execute(String requestJson) {
        ensureOpen();
        try {
            return new MemoryResponse(engine.execute(requestJson));
        } catch (JcMemoryException error) {
            throw mapError(error);
        } catch (RuntimeException error) {
            throw new JiangchengMemoryError("internal", error.getMessage(), null, null, 0);
        }
    }

    /** 写入一个原始 Fragment。 */
    public MemoryResponse observe(String requestJson) {
        return execute(requestJson);
    }

    /** 提交一次完整状态转移。 */
    public MemoryResponse commit(String requestJson) {
        return execute(requestJson);
    }

    /** 执行有界 Recall。 */
    public MemoryResponse recall(String requestJson) {
        return execute(requestJson);
    }

    /** 读取 Scope 统计。 */
    public MemoryResponse stats(String requestJson) {
        return execute(requestJson);
    }

    /** 追加纠正记录。 */
    public MemoryResponse correct(String requestJson) {
        return execute(requestJson);
    }

    /** 追加失效记录。 */
    public MemoryResponse forget(String requestJson) {
        return execute(requestJson);
    }

    /** 返回 Recall 的结构化证据追踪。 */
    public MemoryResponse explain(String requestJson) {
        return execute(requestJson);
    }

    /** 关闭 Rust handle；重复关闭保持幂等，JVM 不执行 native unload。 */
    @Override
    public synchronized void close() {
        if (closed) {
            return;
        }
        try {
            engine.shutdown();
            engine.close();
            closed = true;
        } catch (JcMemoryException error) {
            throw mapError(error);
        }
    }

    private void ensureOpen() {
        if (closed) {
            throw new JiangchengMemoryError("internal", "客户端已经关闭", null, null, 0);
        }
    }

    private static void configureLibrary(Path library) {
        String absolute = library.toAbsolutePath().normalize().toString();
        synchronized (LIBRARY_CONFIGURATION_LOCK) {
            if (configuredLibrary != null && !configuredLibrary.equals(absolute)) {
                throw new NativeLibraryLoadException(
                        "library_conflict",
                        "当前 JVM 已绑定另一个 UniFFI native library",
                        library,
                        null,
                        null,
                        null);
            }
            System.setProperty(LIBRARY_OVERRIDE, absolute);
            configuredLibrary = absolute;
        }
    }

    private static JiangchengMemoryError mapError(JcMemoryException error) {
        if (error instanceof JcMemoryException.Core core) {
            return new JiangchengMemoryError(core.getCode(), core.getDetail(), null, core.getDetail(), 0);
        }
        if (error instanceof JcMemoryException.InvalidArgument invalidArgument) {
            return new JiangchengMemoryError(
                    "invalid_argument", invalidArgument.getDetail(), null, invalidArgument.getDetail(), 0);
        }
        if (error instanceof JcMemoryException.Closed) {
            return new JiangchengMemoryError("internal", "引擎已经关闭", null, null, 0);
        }
        if (error instanceof JcMemoryException.Internal internal) {
            return new JiangchengMemoryError("internal", internal.getDetail(), null, internal.getDetail(), 0);
        }
        return new JiangchengMemoryError("internal", error.getMessage(), null, null, 0);
    }
}
