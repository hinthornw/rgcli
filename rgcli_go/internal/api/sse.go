package api

import (
	"bufio"
	"io"
	"strings"
)

// SSECallback is called for each SSE event
type SSECallback func(event SSEEvent)

// ParseSSE reads an SSE stream and calls the callback for each event
func ParseSSE(reader io.Reader, callback SSECallback) error {
	scanner := bufio.NewScanner(reader)

	var currentEvent SSEEvent
	var dataLines []string

	for scanner.Scan() {
		line := scanner.Text()

		if line == "" {
			// Empty line = end of event
			if len(dataLines) > 0 {
				currentEvent.Data = strings.Join(dataLines, "\n")
				callback(currentEvent)
			}
			// Reset for next event
			currentEvent = SSEEvent{}
			dataLines = nil
			continue
		}

		// Parse SSE field
		if strings.HasPrefix(line, "event:") {
			currentEvent.Event = strings.TrimSpace(strings.TrimPrefix(line, "event:"))
		} else if strings.HasPrefix(line, "data:") {
			data := strings.TrimPrefix(line, "data:")
			// Only trim leading space, not all whitespace
			if len(data) > 0 && data[0] == ' ' {
				data = data[1:]
			}
			dataLines = append(dataLines, data)
		} else if strings.HasPrefix(line, "id:") {
			currentEvent.ID = strings.TrimSpace(strings.TrimPrefix(line, "id:"))
		}
		// Ignore comments (lines starting with :) and unknown fields
	}

	// Handle any remaining event
	if len(dataLines) > 0 {
		currentEvent.Data = strings.Join(dataLines, "\n")
		callback(currentEvent)
	}

	return scanner.Err()
}

// IsEndEvent checks if the event signals end of stream
func IsEndEvent(event SSEEvent) bool {
	return event.Event == "end" || event.Event == "done"
}

// IsMessageEvent checks if the event contains message data
func IsMessageEvent(event SSEEvent) bool {
	return event.Event == "messages" || event.Event == "data"
}
