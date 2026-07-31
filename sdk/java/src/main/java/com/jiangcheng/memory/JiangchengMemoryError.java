package com.jiangcheng.memory;

/** Java SDK 的稳定错误分类。 */
public final class JiangchengMemoryError extends RuntimeException {
    private final String code;
    private final String requestId;
    private final String details;
    private final int status;

    public JiangchengMemoryError(String code, String message, String requestId, String details, int status) {
        super(message);
        this.code = code;
        this.requestId = requestId;
        this.details = details;
        this.status = status;
    }

    public String code() {
        return code;
    }

    public String requestId() {
        return requestId;
    }

    public String details() {
        return details;
    }

    public int status() {
        return status;
    }
}
