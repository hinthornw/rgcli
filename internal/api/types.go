package api

import "encoding/json"

// Message represents a chat message
type Message struct {
	Role    string `json:"role"`
	Content string `json:"content"`
}

// RunRequest is the request body for creating a streaming run
type RunRequest struct {
	AssistantID string                 `json:"assistant_id"`
	Input       map[string]interface{} `json:"input"`
	StreamMode  []string               `json:"stream_mode"`
	IfNotExists string                 `json:"if_not_exists,omitempty"`
}

// Thread represents a thread from the API
type Thread struct {
	ThreadID  string                 `json:"thread_id"`
	CreatedAt string                 `json:"created_at,omitempty"`
	UpdatedAt string                 `json:"updated_at,omitempty"`
	Metadata  map[string]interface{} `json:"metadata,omitempty"`
	Values    map[string]interface{} `json:"values,omitempty"`
}

// ThreadState represents the state of a thread
type ThreadState struct {
	Values     map[string]interface{} `json:"values"`
	Next       []string               `json:"next,omitempty"`
	Checkpoint interface{}            `json:"checkpoint,omitempty"`
}

// SSEEvent represents a parsed SSE event
type SSEEvent struct {
	Event string
	Data  string
	ID    string
}

// ContentBlock represents a content block in a message chunk
type ContentBlock struct {
	Index int    `json:"index"`
	Text  string `json:"text"`
	Type  string `json:"type"`
}

// MessageChunk represents a streamed message chunk from the messages stream mode
type MessageChunk struct {
	Content          any            `json:"content"` // Can be string or []ContentBlock
	Type             string         `json:"type"`
	ID               string         `json:"id,omitempty"`
	Name             string         `json:"name,omitempty"`
	AdditionalKwargs map[string]any `json:"additional_kwargs,omitempty"`
	ResponseMetadata map[string]any `json:"response_metadata,omitempty"`
	ToolCalls        []any          `json:"tool_calls,omitempty"`
	InvalidToolCalls []any          `json:"invalid_tool_calls,omitempty"`
	UsageMetadata    any            `json:"usage_metadata,omitempty"`
}

// GetContent extracts the text content from a MessageChunk
func (m *MessageChunk) GetContent() string {
	if m.Content == nil {
		return ""
	}

	// Try string first
	if s, ok := m.Content.(string); ok {
		return s
	}

	// Try array of content blocks
	if arr, ok := m.Content.([]any); ok {
		var result string
		for _, item := range arr {
			if block, ok := item.(map[string]any); ok {
				if text, ok := block["text"].(string); ok {
					result += text
				}
			}
		}
		return result
	}

	return ""
}

// ParseMessageChunk parses the data field of a messages event
func ParseMessageChunk(data string) (*MessageChunk, error) {
	// Messages mode returns a tuple [chunk, metadata]
	// We need to parse the first element
	var tuple []json.RawMessage
	if err := json.Unmarshal([]byte(data), &tuple); err != nil {
		// Try parsing as a single object
		var chunk MessageChunk
		if err := json.Unmarshal([]byte(data), &chunk); err != nil {
			return nil, err
		}
		return &chunk, nil
	}

	if len(tuple) == 0 {
		return nil, nil
	}

	var chunk MessageChunk
	if err := json.Unmarshal(tuple[0], &chunk); err != nil {
		return nil, err
	}

	return &chunk, nil
}

// NewRunRequest creates a new run request with user message
func NewRunRequest(assistantID, userMessage string) *RunRequest {
	return &RunRequest{
		AssistantID: assistantID,
		Input: map[string]interface{}{
			"messages": []Message{
				{Role: "user", Content: userMessage},
			},
		},
		StreamMode:  []string{"messages-tuple"},
		IfNotExists: "create",
	}
}

// GetMessages extracts messages from thread state values
func GetMessages(values map[string]interface{}) []Message {
	messagesRaw, ok := values["messages"]
	if !ok {
		return nil
	}

	messagesSlice, ok := messagesRaw.([]interface{})
	if !ok {
		return nil
	}

	var messages []Message
	for _, m := range messagesSlice {
		msgMap, ok := m.(map[string]interface{})
		if !ok {
			continue
		}

		role, _ := msgMap["role"].(string)
		// Handle both "role" and "type" fields
		if role == "" {
			msgType, _ := msgMap["type"].(string)
			switch msgType {
			case "human":
				role = "user"
			case "ai":
				role = "assistant"
			default:
				role = msgType
			}
		}

		content, _ := msgMap["content"].(string)
		if role != "" && content != "" {
			messages = append(messages, Message{Role: role, Content: content})
		}
	}

	return messages
}
