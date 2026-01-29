package api

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"log"
	"net/http"
	"strings"

	"github.com/wfh/lsc/internal/config"
)

// Client is the LangSmith Agent Server API client
type Client struct {
	endpoint   string
	headers    map[string]string
	httpClient *http.Client
}

// NewClient creates a new API client from config
func NewClient(cfg *config.Config) *Client {
	return &Client{
		endpoint:   strings.TrimSuffix(cfg.Endpoint, "/"),
		headers:    cfg.GetHeaders(),
		httpClient: &http.Client{},
	}
}

// CreateThread creates a new thread
func (c *Client) CreateThread(ctx context.Context) (*Thread, error) {
	url := fmt.Sprintf("%s/threads", c.endpoint)

	req, err := http.NewRequestWithContext(ctx, "POST", url, bytes.NewReader([]byte("{}")))
	if err != nil {
		return nil, err
	}

	for k, v := range c.headers {
		req.Header.Set(k, v)
	}

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return nil, err
	}
	defer func() {
		if err := resp.Body.Close(); err != nil {
			log.Printf("error closing response body: %v", err)
		}
	}()

	if resp.StatusCode != http.StatusOK && resp.StatusCode != http.StatusCreated {
		body, _ := io.ReadAll(resp.Body)
		return nil, fmt.Errorf("failed to create thread: %s - %s", resp.Status, string(body))
	}

	var thread Thread
	if err := json.NewDecoder(resp.Body).Decode(&thread); err != nil {
		return nil, err
	}

	return &thread, nil
}

// SearchThreads searches for existing threads
func (c *Client) SearchThreads(ctx context.Context, limit int) ([]Thread, error) {
	url := fmt.Sprintf("%s/threads/search", c.endpoint)

	body := map[string]interface{}{
		"limit": limit,
	}
	bodyBytes, err := json.Marshal(body)
	if err != nil {
		return nil, err
	}

	req, err := http.NewRequestWithContext(ctx, "POST", url, bytes.NewReader(bodyBytes))
	if err != nil {
		return nil, err
	}

	for k, v := range c.headers {
		req.Header.Set(k, v)
	}

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return nil, err
	}
	defer func() {
		if err := resp.Body.Close(); err != nil {
			log.Printf("error closing response body: %v", err)
		}
	}()

	if resp.StatusCode != http.StatusOK {
		body, _ := io.ReadAll(resp.Body)
		return nil, fmt.Errorf("failed to search threads: %s - %s", resp.Status, string(body))
	}

	var threads []Thread
	if err := json.NewDecoder(resp.Body).Decode(&threads); err != nil {
		return nil, err
	}

	return threads, nil
}

// GetThreadState gets the current state of a thread
func (c *Client) GetThreadState(ctx context.Context, threadID string) (*ThreadState, error) {
	url := fmt.Sprintf("%s/threads/%s/state", c.endpoint, threadID)

	req, err := http.NewRequestWithContext(ctx, "GET", url, nil)
	if err != nil {
		return nil, err
	}

	for k, v := range c.headers {
		req.Header.Set(k, v)
	}

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return nil, err
	}
	defer func() {
		if err := resp.Body.Close(); err != nil {
			log.Printf("error closing response body: %v", err)
		}
	}()

	if resp.StatusCode != http.StatusOK {
		body, _ := io.ReadAll(resp.Body)
		return nil, fmt.Errorf("failed to get thread state: %s - %s", resp.Status, string(body))
	}

	var state ThreadState
	if err := json.NewDecoder(resp.Body).Decode(&state); err != nil {
		return nil, err
	}

	return &state, nil
}

// GetThread gets a thread with optional field selection
func (c *Client) GetThread(ctx context.Context, threadID string, selectFields ...string) (*Thread, error) {
	url := fmt.Sprintf("%s/threads/%s", c.endpoint, threadID)
	if len(selectFields) > 0 {
		url += "?select=" + strings.Join(selectFields, ",")
	}

	req, err := http.NewRequestWithContext(ctx, "GET", url, nil)
	if err != nil {
		return nil, err
	}

	for k, v := range c.headers {
		req.Header.Set(k, v)
	}

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return nil, err
	}
	defer func() {
		if err := resp.Body.Close(); err != nil {
			log.Printf("error closing response body: %v", err)
		}
	}()

	if resp.StatusCode != http.StatusOK {
		body, _ := io.ReadAll(resp.Body)
		return nil, fmt.Errorf("failed to get thread: %s - %s", resp.Status, string(body))
	}

	var thread Thread
	if err := json.NewDecoder(resp.Body).Decode(&thread); err != nil {
		return nil, err
	}

	return &thread, nil
}

// TokenCallback is called for each token received
type TokenCallback func(token string)

// StreamRun creates a streaming run and calls the callback for each token
func (c *Client) StreamRun(ctx context.Context, threadID string, assistantID string, userMessage string, onToken TokenCallback) error {
	url := fmt.Sprintf("%s/threads/%s/runs/stream", c.endpoint, threadID)

	runReq := NewRunRequest(assistantID, userMessage)
	bodyBytes, err := json.Marshal(runReq)
	if err != nil {
		return err
	}

	req, err := http.NewRequestWithContext(ctx, "POST", url, bytes.NewReader(bodyBytes))
	if err != nil {
		return err
	}

	for k, v := range c.headers {
		req.Header.Set(k, v)
	}
	req.Header.Set("Accept", "text/event-stream")

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return err
	}
	defer func() {
		if err := resp.Body.Close(); err != nil {
			log.Printf("error closing response body: %v", err)
		}
	}()

	if resp.StatusCode != http.StatusOK {
		body, _ := io.ReadAll(resp.Body)
		return fmt.Errorf("failed to create run: %s - %s", resp.Status, string(body))
	}

	// Parse SSE stream
	return ParseSSE(resp.Body, func(event SSEEvent) {
		if IsEndEvent(event) {
			return
		}

		if !IsMessageEvent(event) && event.Event != "" {
			return
		}

		// Parse message chunk
		chunk, err := ParseMessageChunk(event.Data)
		if err != nil || chunk == nil {
			return
		}

		// Only emit content from AI message chunks
		content := chunk.GetContent()
		if content != "" && (chunk.Type == "AIMessageChunk" || chunk.Type == "ai") {
			onToken(content)
		}
	})
}
