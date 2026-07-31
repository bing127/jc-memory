package memory

import (
	"errors"
	"fmt"
)

// Error 是 Go SDK 的稳定错误类型。
type Error struct {
	CodeValue      string
	RequestIDValue string
	Message        string
	Details        any
	Status         int
}

func (err *Error) Error() string {
	if err.RequestIDValue == "" {
		return fmt.Sprintf("%s: %s", err.CodeValue, err.Message)
	}
	return fmt.Sprintf("%s: %s (request_id=%s)", err.CodeValue, err.Message, err.RequestIDValue)
}

// Code 返回稳定错误分类；非 SDK 错误返回 internal。
func Code(err error) string {
	var target *Error
	if errors.As(err, &target) {
		return target.CodeValue
	}
	return "internal"
}

// RequestID 返回错误关联的请求标识。
func RequestID(err error) string {
	var target *Error
	if errors.As(err, &target) {
		return target.RequestIDValue
	}
	return ""
}
