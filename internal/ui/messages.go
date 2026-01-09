package ui

import (
	"fmt"
	"strings"

	"github.com/wfh/lsc/internal/api"
)

// FormatMessage formats a message for display
func FormatMessage(msg api.Message) string {
	switch msg.Role {
	case "user", "human":
		return fmt.Sprintf("%s%s", UserLabel, msg.Content)
	case "assistant", "ai":
		return fmt.Sprintf("%s%s", AssistantLabel, msg.Content)
	default:
		return fmt.Sprintf("[%s] %s", msg.Role, msg.Content)
	}
}

// FormatHistory formats a list of messages for display
func FormatHistory(messages []api.Message) string {
	var sb strings.Builder
	for i, msg := range messages {
		sb.WriteString(FormatMessage(msg))
		if i < len(messages)-1 {
			sb.WriteString("\n\n")
		}
	}
	return sb.String()
}

// PrintSystem prints a system message
func PrintSystem(msg string) string {
	return SystemStyle.Render(msg)
}

// PrintError prints an error message
func PrintError(msg string) string {
	return ErrorStyle.Render("Error: " + msg)
}

// GetThreadPreview returns a preview string for a thread
func GetThreadPreview(thread api.Thread) string {
	messages := api.GetMessages(thread.Values)
	if len(messages) == 0 {
		return "(empty)"
	}

	// Get first user message as preview
	for _, msg := range messages {
		if msg.Role == "user" || msg.Role == "human" {
			preview := msg.Content
			if len(preview) > 50 {
				preview = preview[:47] + "..."
			}
			return fmt.Sprintf("\"%s\"", preview)
		}
	}

	// Fallback to first message
	preview := messages[0].Content
	if len(preview) > 50 {
		preview = preview[:47] + "..."
	}
	return fmt.Sprintf("\"%s\"", preview)
}
