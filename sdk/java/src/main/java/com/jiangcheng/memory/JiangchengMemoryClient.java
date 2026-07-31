package com.jiangcheng.memory;

import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.net.http.HttpTimeoutException;
import java.time.Duration;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.CompletionException;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

/** 使用 JDK HttpClient 的异步远程客户端。 */
public final class JiangchengMemoryClient implements AutoCloseable {
    private static final Pattern CODE = Pattern.compile("\\\"code\\\"\\s*:\\s*\\\"([^\\\"]*)\\\"");
    private static final Pattern MESSAGE = Pattern.compile("\\\"message\\\"\\s*:\\s*\\\"([^\\\"]*)\\\"");
    private static final Pattern REQUEST_ID = Pattern.compile("\\\"request_id\\\"\\s*:\\s*\\\"([^\\\"]*)\\\"");

    private final String baseUrl;
    private final String apiKey;
    private final HttpClient httpClient;
    private final Duration deadline;
    private volatile boolean closed;

    public JiangchengMemoryClient(String baseUrl) {
        this(baseUrl, null, HttpClient.newHttpClient(), Duration.ofSeconds(30));
    }

    public JiangchengMemoryClient(String baseUrl, String apiKey, HttpClient httpClient) {
        this(baseUrl, apiKey, httpClient, Duration.ofSeconds(30));
    }

    public JiangchengMemoryClient(String baseUrl, String apiKey, HttpClient httpClient, Duration deadline) {
        this.baseUrl = baseUrl == null ? "" : baseUrl.replaceAll("/+$", "");
        this.apiKey = apiKey;
        this.httpClient = httpClient;
        this.deadline = deadline;
    }

    public CompletableFuture<MemoryResponse> observe(String requestJson) {
        return execute("/v1/memory/observe", requestJson);
    }

    public CompletableFuture<MemoryResponse> commit(String requestJson) {
        return execute("/v1/memory/commit", requestJson);
    }

    public CompletableFuture<MemoryResponse> recall(String requestJson) {
        return execute("/v1/memory/recall", requestJson);
    }

    public CompletableFuture<MemoryResponse> stats(String requestJson) {
        return execute("/v1/memory/stats", requestJson);
    }

    public CompletableFuture<MemoryResponse> correct(String requestJson) {
        return execute("/v1/memory/correct", requestJson);
    }

    public CompletableFuture<MemoryResponse> forget(String requestJson) {
        return execute("/v1/memory/forget", requestJson);
    }

    public CompletableFuture<MemoryResponse> explain(String requestJson) {
        return execute("/v1/memory/explain", requestJson);
    }

    /** 阻塞等待 Observe 完成，业务错误不会被额外包装。 */
    public MemoryResponse observeBlocking(String requestJson) {
        return block(observe(requestJson));
    }

    /** 阻塞等待 Commit 完成，业务错误不会被额外包装。 */
    public MemoryResponse commitBlocking(String requestJson) {
        return block(commit(requestJson));
    }

    /** 阻塞等待 Recall 完成，业务错误不会被额外包装。 */
    public MemoryResponse recallBlocking(String requestJson) {
        return block(recall(requestJson));
    }

    /** 阻塞等待 Stats 完成，业务错误不会被额外包装。 */
    public MemoryResponse statsBlocking(String requestJson) {
        return block(stats(requestJson));
    }

    /** 阻塞等待 Correct 完成，业务错误不会被额外包装。 */
    public MemoryResponse correctBlocking(String requestJson) {
        return block(correct(requestJson));
    }

    /** 阻塞等待 Forget 完成，业务错误不会被额外包装。 */
    public MemoryResponse forgetBlocking(String requestJson) {
        return block(forget(requestJson));
    }

    /** 阻塞等待 Explain 完成，业务错误不会被额外包装。 */
    public MemoryResponse explainBlocking(String requestJson) {
        return block(explain(requestJson));
    }

    @Override
    public void close() {
        closed = true;
    }

    private CompletableFuture<MemoryResponse> execute(String path, String requestJson) {
        if (closed) {
            return CompletableFuture.failedFuture(new JiangchengMemoryError("internal", "客户端已经关闭", null, null, 0));
        }
        if (baseUrl.isBlank()) {
            return CompletableFuture.failedFuture(new JiangchengMemoryError("invalid_argument", "baseUrl 不能为空", null, null, 0));
        }
        HttpRequest.Builder builder = HttpRequest.newBuilder()
                .uri(URI.create(baseUrl + path))
                .timeout(deadline)
                .header("content-type", "application/json")
                .header("accept", "application/json")
                .POST(HttpRequest.BodyPublishers.ofString(requestJson));
        if (apiKey != null && !apiKey.isBlank()) {
            builder.header("authorization", "Bearer " + apiKey);
        }
        CompletableFuture<HttpResponse<String>> transport = httpClient.sendAsync(
                builder.build(), HttpResponse.BodyHandlers.ofString());
        CompletableFuture<MemoryResponse> result = new CompletableFuture<>();
        transport.whenComplete((response, failure) -> {
            if (failure != null) {
                Throwable cause = failure instanceof CompletionException ? failure.getCause() : failure;
                if (cause instanceof HttpTimeoutException) {
                    result.completeExceptionally(new JiangchengMemoryError("deadline_exceeded", "请求超过 deadline", null, null, 0));
                } else if (!result.isCancelled()) {
                    result.completeExceptionally(new JiangchengMemoryError("transport", cause.getMessage(), null, null, 0));
                }
                return;
            }
            if (response.statusCode() < 200 || response.statusCode() >= 300) {
                result.completeExceptionally(parseError(response));
            } else {
                result.complete(new MemoryResponse(response.body()));
            }
        });
        result.whenComplete((ignored, failure) -> {
            if (result.isCancelled()) {
                transport.cancel(true);
            }
        });
        return result;
    }

    private static MemoryResponse block(CompletableFuture<MemoryResponse> future) {
        try {
            return future.join();
        } catch (CompletionException error) {
            if (error.getCause() instanceof RuntimeException runtimeError) {
                throw runtimeError;
            }
            throw error;
        }
    }

    private JiangchengMemoryError parseError(HttpResponse<String> response) {
        String body = response.body();
        String code = match(CODE, body);
        if (code == null) code = codeForStatus(response.statusCode());
        String message = match(MESSAGE, body);
        if (message == null) message = "HTTP " + response.statusCode();
        return new JiangchengMemoryError(code, message, match(REQUEST_ID, body), body, response.statusCode());
    }

    private static String match(Pattern pattern, String value) {
        Matcher matcher = pattern.matcher(value);
        return matcher.find() ? matcher.group(1) : null;
    }

    private static String codeForStatus(int status) {
        return switch (status) {
            case 401 -> "unauthorized";
            case 403 -> "permission_denied";
            case 404 -> "not_found";
            case 408, 504 -> "deadline_exceeded";
            case 409 -> "conflict";
            case 429 -> "resource_exhausted";
            default -> status >= 400 && status < 500 ? "invalid_argument" : "internal";
        };
    }
}
