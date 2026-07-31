package memory_test

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"reflect"
	"testing"
	"time"

	"github.com/bing127/jc-memory/sdk/go/memory"
)

type fixture struct {
	Request  map[string]any `json:"request"`
	Response map[string]any `json:"response"`
}

func loadFixture(t *testing.T, name string) fixture {
	t.Helper()
	path := filepath.Join("..", "..", "contracts", "fixtures", name)
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	var value fixture
	if err := json.Unmarshal(data, &value); err != nil {
		t.Fatal(err)
	}
	return value
}

func TestClientReturnsCanonicalRecallPacket(t *testing.T) {
	value := loadFixture(t, "recall-exact.json")
	server := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		if request.URL.Path != "/v1/memory/recall" {
			t.Errorf("unexpected path %s", request.URL.Path)
		}
		if request.Header.Get("Authorization") != "Bearer fixture" {
			t.Errorf("missing authorization header")
		}
		var body map[string]any
		if err := json.NewDecoder(request.Body).Decode(&body); err != nil {
			t.Fatal(err)
		}
		if !jsonEqual(body, value.Request) {
			t.Errorf("request body differs from fixture")
		}
		writer.Header().Set("content-type", "application/json")
		_ = json.NewEncoder(writer).Encode(value.Response)
	}))
	defer server.Close()

	client := memory.NewClient(server.URL, "fixture", nil)
	request := mustRequest(t, value.Request)
	response, err := client.Recall(context.Background(), request)
	if err != nil {
		t.Fatal(err)
	}
	if !jsonEqual(response.Raw(), value.Response) {
		t.Errorf("response differs from fixture")
	}
}

func TestClientMapsHTTPError(t *testing.T) {
	value := loadFixture(t, "recall-exact.json")
	server := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, _ *http.Request) {
		writer.Header().Set("content-type", "application/json")
		writer.WriteHeader(http.StatusBadRequest)
		_, _ = writer.Write([]byte(`{"code":"invalid_argument","message":"bad request","request_id":"fixture-recall-exact"}`))
	}))
	defer server.Close()

	client := memory.NewClient(server.URL, "fixture", nil)
	_, err := client.Recall(context.Background(), mustRequest(t, value.Request))
	if err == nil {
		t.Fatal("expected error")
	}
	if got := memory.Code(err); got != "invalid_argument" || memory.RequestID(err) != "fixture-recall-exact" {
		t.Fatalf("unexpected error: code=%s request_id=%s", got, memory.RequestID(err))
	}
}

func TestClientHonorsContextCancellation(t *testing.T) {
	started := make(chan struct{})
	server := httptest.NewServer(http.HandlerFunc(func(_ http.ResponseWriter, request *http.Request) {
		close(started)
		<-request.Context().Done()
	}))
	defer server.Close()

	client := memory.NewClient(server.URL, "fixture", nil)
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	value := loadFixture(t, "recall-exact.json")
	done := make(chan error, 1)
	go func() {
		_, err := client.Recall(ctx, mustRequest(t, value.Request))
		done <- err
	}()
	<-started
	cancel()
	select {
	case err := <-done:
		if err == nil {
			t.Fatal("expected cancellation error")
		}
	case <-time.After(2 * time.Second):
		t.Fatal("request did not honor cancellation")
	}
}

func mustRequest(t *testing.T, value map[string]any) memory.RequestEnvelope {
	t.Helper()
	data, err := json.Marshal(value)
	if err != nil {
		t.Fatal(err)
	}
	request, err := memory.ParseRequest(data)
	if err != nil {
		t.Fatal(err)
	}
	return request
}

func jsonEqual(left, right any) bool {
	leftData, _ := json.Marshal(left)
	rightData, _ := json.Marshal(right)
	var leftValue, rightValue any
	_ = json.Unmarshal(leftData, &leftValue)
	_ = json.Unmarshal(rightData, &rightValue)
	return reflect.DeepEqual(leftValue, rightValue)
}
