package memory

import (
	"bytes"
	"encoding/json"
	"fmt"
)

// RequestEnvelope 保留跨语言协议的原始 JSON 字段。
type RequestEnvelope struct {
	APIVersion  string          `json:"api_version"`
	RequestID   string          `json:"request_id"`
	AccessScope json.RawMessage `json:"access_scope"`
	Command     json.RawMessage `json:"command"`
}

// ResponseEnvelope 保留服务端响应 payload 的原始 JSON 表示。
type ResponseEnvelope struct {
	APIVersion string          `json:"api_version"`
	RequestID  string          `json:"request_id"`
	Response   json.RawMessage `json:"response"`
}

// ParseRequest 解析并检查最小 Request Envelope 字段。
func ParseRequest(data []byte) (RequestEnvelope, error) {
	var request RequestEnvelope
	if err := json.Unmarshal(data, &request); err != nil {
		return RequestEnvelope{}, fmt.Errorf("invalid request envelope: %w", err)
	}
	if request.APIVersion == "" || request.RequestID == "" || len(request.AccessScope) == 0 || len(request.Command) == 0 {
		return RequestEnvelope{}, fmt.Errorf("invalid request envelope: missing required field")
	}
	return request, nil
}

// Raw 将响应解码为可供调用方继续类型化的 JSON 值。
func (response ResponseEnvelope) Raw() any {
	var value any
	if err := json.Unmarshal(responseBytes(response), &value); err != nil {
		return nil
	}
	return value
}

func responseBytes(response ResponseEnvelope) []byte {
	data, _ := json.Marshal(response)
	return bytes.Clone(data)
}
