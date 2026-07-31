package memory

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"strings"
	"sync"
)

// Client 是带 Context 取消语义的远程 HTTP 客户端。
type Client struct {
	baseURL string
	apiKey  string
	http    *http.Client
	mu      sync.RWMutex
	closed  bool
}

// NewClient 创建远程客户端；httpClient 为 nil 时使用 http.DefaultClient。
func NewClient(baseURL, apiKey string, httpClient *http.Client) *Client {
	if httpClient == nil {
		httpClient = http.DefaultClient
	}
	return &Client{baseURL: strings.TrimRight(baseURL, "/"), apiKey: apiKey, http: httpClient}
}

// Observe 写入一个原始 Fragment。
func (client *Client) Observe(ctx context.Context, request RequestEnvelope) (ResponseEnvelope, error) {
	return client.execute(ctx, "/v1/memory/observe", request)
}

// Commit 提交一次完整状态转移的 Fragment 集合。
func (client *Client) Commit(ctx context.Context, request RequestEnvelope) (ResponseEnvelope, error) {
	return client.execute(ctx, "/v1/memory/commit", request)
}

// Recall 执行有界 Recall。
func (client *Client) Recall(ctx context.Context, request RequestEnvelope) (ResponseEnvelope, error) {
	return client.execute(ctx, "/v1/memory/recall", request)
}

// Stats 读取当前 Scope 统计。
func (client *Client) Stats(ctx context.Context, request RequestEnvelope) (ResponseEnvelope, error) {
	return client.execute(ctx, "/v1/memory/stats", request)
}

// Correct 追加纠正记录。
func (client *Client) Correct(ctx context.Context, request RequestEnvelope) (ResponseEnvelope, error) {
	return client.execute(ctx, "/v1/memory/correct", request)
}

// Forget 追加失效记录。
func (client *Client) Forget(ctx context.Context, request RequestEnvelope) (ResponseEnvelope, error) {
	return client.execute(ctx, "/v1/memory/forget", request)
}

// Explain 返回 Recall 的结构化证据追踪。
func (client *Client) Explain(ctx context.Context, request RequestEnvelope) (ResponseEnvelope, error) {
	return client.execute(ctx, "/v1/memory/explain", request)
}

// Close 关闭客户端；HTTP Transport 的连接由其自身管理。
func (client *Client) Close() error {
	client.mu.Lock()
	defer client.mu.Unlock()
	client.closed = true
	return nil
}

func (client *Client) execute(ctx context.Context, path string, request RequestEnvelope) (ResponseEnvelope, error) {
	if ctx == nil {
		ctx = context.Background()
	}
	client.mu.RLock()
	closed := client.closed
	client.mu.RUnlock()
	if closed {
		return ResponseEnvelope{}, &Error{CodeValue: "internal", RequestIDValue: request.RequestID, Message: "client is closed"}
	}
	if client.baseURL == "" {
		return ResponseEnvelope{}, &Error{CodeValue: "invalid_argument", RequestIDValue: request.RequestID, Message: "base URL is empty"}
	}
	body, err := json.Marshal(request)
	if err != nil {
		return ResponseEnvelope{}, &Error{CodeValue: "invalid_argument", RequestIDValue: request.RequestID, Message: err.Error()}
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, client.baseURL+path, strings.NewReader(string(body)))
	if err != nil {
		return ResponseEnvelope{}, &Error{CodeValue: "invalid_argument", RequestIDValue: request.RequestID, Message: err.Error()}
	}
	req.Header.Set("content-type", "application/json")
	req.Header.Set("accept", "application/json")
	if client.apiKey != "" {
		req.Header.Set("authorization", "Bearer "+client.apiKey)
	}
	response, err := client.http.Do(req)
	if err != nil {
		if errors.Is(ctx.Err(), context.Canceled) {
			return ResponseEnvelope{}, &Error{CodeValue: "cancelled", RequestIDValue: request.RequestID, Message: "request cancelled"}
		}
		if errors.Is(ctx.Err(), context.DeadlineExceeded) {
			return ResponseEnvelope{}, &Error{CodeValue: "deadline_exceeded", RequestIDValue: request.RequestID, Message: "request deadline exceeded"}
		}
		return ResponseEnvelope{}, &Error{CodeValue: "transport", RequestIDValue: request.RequestID, Message: err.Error(), Details: err}
	}
	defer response.Body.Close()
	responseBody, readErr := io.ReadAll(response.Body)
	if readErr != nil {
		return ResponseEnvelope{}, &Error{CodeValue: "transport", RequestIDValue: request.RequestID, Message: readErr.Error(), Details: readErr}
	}
	if response.StatusCode < http.StatusOK || response.StatusCode >= http.StatusMultipleChoices {
		return ResponseEnvelope{}, decodeHTTPError(response.StatusCode, responseBody, request.RequestID)
	}
	var result ResponseEnvelope
	if err := json.Unmarshal(responseBody, &result); err != nil {
		return ResponseEnvelope{}, &Error{CodeValue: "internal", RequestIDValue: request.RequestID, Message: fmt.Sprintf("invalid response envelope: %v", err)}
	}
	return result, nil
}

func decodeHTTPError(status int, body []byte, fallbackRequestID string) error {
	var value struct {
		Code      string `json:"code"`
		Message   string `json:"message"`
		RequestID string `json:"request_id"`
	}
	_ = json.Unmarshal(body, &value)
	code := value.Code
	if code == "" {
		switch status {
		case http.StatusUnauthorized:
			code = "unauthorized"
		case http.StatusForbidden:
			code = "permission_denied"
		case http.StatusNotFound:
			code = "not_found"
		case http.StatusConflict:
			code = "conflict"
		case http.StatusRequestTimeout, http.StatusGatewayTimeout:
			code = "deadline_exceeded"
		case http.StatusTooManyRequests:
			code = "resource_exhausted"
		default:
			code = "invalid_argument"
			if status >= http.StatusInternalServerError {
				code = "internal"
			}
		}
	}
	if value.RequestID == "" {
		value.RequestID = fallbackRequestID
	}
	if value.Message == "" {
		value.Message = fmt.Sprintf("HTTP %d", status)
	}
	return &Error{CodeValue: code, RequestIDValue: value.RequestID, Message: value.Message, Status: status}
}
