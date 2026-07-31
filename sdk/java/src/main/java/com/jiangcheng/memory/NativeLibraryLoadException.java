package com.jiangcheng.memory;

import java.nio.file.Path;

/** Native library 加载边界的稳定错误。 */
public final class NativeLibraryLoadException extends RuntimeException {
    private final String code;
    private final Path path;
    private final String expectedSha256;
    private final String actualSha256;

    public NativeLibraryLoadException(
            String code,
            String message,
            Path path,
            String expectedSha256,
            String actualSha256,
            Throwable cause) {
        super(message, cause);
        this.code = code;
        this.path = path;
        this.expectedSha256 = expectedSha256;
        this.actualSha256 = actualSha256;
    }

    public String code() {
        return code;
    }

    public Path path() {
        return path;
    }

    public String expectedSha256() {
        return expectedSha256;
    }

    public String actualSha256() {
        return actualSha256;
    }
}
