package main

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"

	"github.com/bing127/jc-memory/sdk/go/memory"
)

func main() {
	fixturePath := filepath.Join("..", "..", "sdk", "contracts", "fixtures", "recall-exact.json")
	data, err := os.ReadFile(fixturePath)
	if err != nil {
		panic(err)
	}
	var fixture struct {
		Request map[string]any `json:"request"`
	}
	if err := json.Unmarshal(data, &fixture); err != nil {
		panic(err)
	}
	requestData, err := json.Marshal(fixture.Request)
	if err != nil {
		panic(err)
	}
	request, err := memory.ParseRequest(requestData)
	if err != nil {
		panic(err)
	}
	client := memory.NewClient(os.Getenv("JC_MEMORY_URL"), os.Getenv("JC_MEMORY_API_KEY"), nil)
	defer client.Close()
	response, err := client.Recall(context.Background(), request)
	if err != nil {
		panic(err)
	}
	responseData, err := json.Marshal(response.Raw())
	if err != nil {
		panic(err)
	}
	fmt.Println(string(responseData))
}
