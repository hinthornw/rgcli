package ui

import (
	"context"
	"fmt"
	"strings"
	"time"

	"github.com/charmbracelet/bubbles/spinner"
	"github.com/charmbracelet/bubbles/textarea"
	"github.com/charmbracelet/bubbles/textinput"
	tea "github.com/charmbracelet/bubbletea"
	"github.com/charmbracelet/lipgloss"

	"github.com/wfh/lsc/internal/api"
)

// State represents the current UI state
type State int

const (
	StateInput State = iota
	StateWaiting
	StateStreaming
)

// Model is the main Bubbletea model for the chat interface
type Model struct {
	client      *api.Client
	assistantID string
	threadID    string

	textInput textinput.Model
	spinner   spinner.Model

	state           State
	currentResponse strings.Builder
	output          strings.Builder // Accumulated output to print
	err             error
	quitting        bool
}

// TokenMsg is sent when a token is received from the stream
type TokenMsg struct {
	Token string
}

// StreamDoneMsg is sent when the stream is complete
type StreamDoneMsg struct {
	Err error
}

// NewModel creates a new chat model
func NewModel(client *api.Client, assistantID, threadID string) Model {
	ti := textinput.New()
	ti.Placeholder = "Type a message..."
	ti.Focus()
	ti.CharLimit = 0 // No limit
	ti.Width = 80
	ti.Prompt = PromptStyle.Render("> ")

	s := spinner.New()
	s.Spinner = spinner.Dot
	s.Style = SpinnerStyle

	return Model{
		client:      client,
		assistantID: assistantID,
		threadID:    threadID,
		textInput:   ti,
		spinner:     s,
		state:       StateInput,
	}
}

// Init initializes the model
func (m Model) Init() tea.Cmd {
	return textinput.Blink
}

// Update handles messages
func (m Model) Update(msg tea.Msg) (tea.Model, tea.Cmd) {
	var cmds []tea.Cmd

	switch msg := msg.(type) {
	case tea.KeyMsg:
		switch msg.Type {
		case tea.KeyCtrlC, tea.KeyCtrlD:
			m.quitting = true
			return m, tea.Quit

		case tea.KeyEnter:
			if m.state != StateInput {
				return m, nil
			}

			input := strings.TrimSpace(m.textInput.Value())
			if input == "" {
				return m, nil
			}

			// Handle commands
			if input == "/quit" || input == "/exit" {
				m.quitting = true
				return m, tea.Quit
			}

			if input == "/configure" {
				// Signal to main to run configure
				m.output.WriteString("\n")
				m.quitting = true
				m.err = fmt.Errorf("CONFIGURE")
				return m, tea.Quit
			}

			// Print user message and start streaming
			m.output.WriteString(fmt.Sprintf("%s%s\n", UserLabel, input))
			m.textInput.Reset()
			m.state = StateWaiting
			m.currentResponse.Reset()

			return m, tea.Batch(
				m.spinner.Tick,
				m.startStream(input),
			)
		}

	case spinner.TickMsg:
		if m.state == StateWaiting {
			var cmd tea.Cmd
			m.spinner, cmd = m.spinner.Update(msg)
			return m, cmd
		}

	case TokenMsg:
		if m.state == StateWaiting {
			// First token - clear spinner and switch to streaming
			m.state = StateStreaming
			m.output.WriteString(AssistantLabel)
		}
		m.currentResponse.WriteString(msg.Token)
		m.output.WriteString(msg.Token)
		return m, nil

	case StreamDoneMsg:
		if msg.Err != nil {
			m.output.WriteString("\n")
			m.output.WriteString(PrintError(msg.Err.Error()))
		}
		m.output.WriteString("\n\n")
		m.state = StateInput
		return m, textinput.Blink
	}

	// Update text input
	if m.state == StateInput {
		var cmd tea.Cmd
		m.textInput, cmd = m.textInput.Update(msg)
		cmds = append(cmds, cmd)
	}

	return m, tea.Batch(cmds...)
}

// View renders the UI
func (m Model) View() string {
	if m.quitting {
		out := m.output.String()
		if m.err == nil || m.err.Error() != "CONFIGURE" {
			out += "Goodbye!\n"
		}
		return out
	}

	var sb strings.Builder

	// Print accumulated output
	sb.WriteString(m.output.String())

	// Show current state
	switch m.state {
	case StateInput:
		sb.WriteString(m.textInput.View())
	case StateWaiting:
		sb.WriteString(m.spinner.View())
		sb.WriteString(" Thinking...")
	case StateStreaming:
		// Content is already in output
	}

	return sb.String()
}

// startStream starts the streaming API call
func (m *Model) startStream(userMessage string) tea.Cmd {
	return func() tea.Msg {
		ctx := context.Background()

		err := m.client.StreamRun(ctx, m.threadID, m.assistantID, userMessage, func(token string) {
			// Send token to the model
			// Note: This is a bit tricky with bubbletea - we'll use a channel approach
		})

		return StreamDoneMsg{Err: err}
	}
}

// StreamChat runs the chat with streaming in a way that works with bubbletea
// This is called from main to set up the program properly
func StreamChat(client *api.Client, assistantID, threadID string, history []api.Message) error {
	// Print history if any
	if len(history) > 0 {
		fmt.Println(FormatHistory(history))
		fmt.Println()
	}

	p := tea.NewProgram(NewModel(client, assistantID, threadID))

	// Run with a custom approach for streaming
	model, err := runWithStreaming(p, client, assistantID, threadID)
	if err != nil {
		return err
	}

	// Check if we need to reconfigure
	if m, ok := model.(Model); ok && m.err != nil && m.err.Error() == "CONFIGURE" {
		return m.err
	}

	return nil
}

// runWithStreaming runs the bubbletea program with streaming support
func runWithStreaming(p *tea.Program, client *api.Client, assistantID, threadID string) (tea.Model, error) {
	// We need a different approach - use a simple input/output loop instead
	// of bubbletea for better streaming support
	return nil, fmt.Errorf("use RunChatLoop instead")
}

// RunChatLoop runs a simple chat loop with streaming
// This bypasses bubbletea for simplicity and better streaming support
func RunChatLoop(client *api.Client, assistantID, threadID string, history []api.Message) error {
	// Print history if any
	if len(history) > 0 {
		fmt.Println(FormatHistory(history))
		fmt.Println()
	}

	s := spinner.New()
	s.Spinner = spinner.Dot
	s.Style = SpinnerStyle

	for {
		// Create a textarea for multiline input
		ta := textarea.New()
		ta.Placeholder = "Type a message... (Shift+Enter for newline)"
		ta.Focus()
		ta.CharLimit = 0
		ta.ShowLineNumbers = false
		ta.Prompt = PromptStyle.Render("> ")
		ta.SetWidth(80)
		ta.SetHeight(1)

		// Remove default styling (no background color)
		ta.FocusedStyle.Base = lipgloss.NewStyle()
		ta.BlurredStyle.Base = lipgloss.NewStyle()
		ta.FocusedStyle.CursorLine = lipgloss.NewStyle()
		ta.BlurredStyle.CursorLine = lipgloss.NewStyle()
		ta.FocusedStyle.Placeholder = SystemStyle
		ta.BlurredStyle.Placeholder = SystemStyle

		// Create a simple input program
		inModel := inputModel{textarea: ta}
		p := tea.NewProgram(inModel)
		finalModel, err := p.Run()
		if err != nil {
			return err
		}

		resultModel := finalModel.(inputModel)
		if resultModel.quitting {
			fmt.Println("Goodbye!")
			return nil
		}

		input := resultModel.value
		if input == "/configure" {
			return fmt.Errorf("CONFIGURE")
		}

		// Print user message (show newlines properly)
		fmt.Printf("%s%s\n", UserLabel, input)

		// Show spinner and stream response
		ctx := context.Background()
		fmt.Print(s.View() + " Thinking...")

		firstToken := true
		err = client.StreamRun(ctx, threadID, assistantID, input, func(token string) {
			if firstToken {
				// Clear spinner line and print assistant label
				fmt.Print("\r\033[K") // Clear line
				fmt.Print(AssistantLabel)
				firstToken = false
			}
			fmt.Print(token)
		})

		if firstToken {
			// No tokens received, clear spinner
			fmt.Print("\r\033[K")
		}

		if err != nil {
			fmt.Println()
			fmt.Println(PrintError(err.Error()))
		} else {
			// Fetch final thread state to get the complete conversation
			thread, err := client.GetThread(ctx, threadID, "values")
			if err == nil && thread.Values != nil {
				// Thread values contain the full message history
				_ = api.GetMessages(thread.Values)
			}
		}
		fmt.Println()
		fmt.Println()
	}
}

// Available slash commands
var slashCommands = []struct {
	name string
	desc string
}{
	{"/configure", "Update connection settings"},
	{"/quit", "Exit the chat"},
	{"/exit", "Exit the chat"},
}

// inputModel is a simple model just for getting user input
type inputModel struct {
	textarea      textarea.Model
	value         string
	quitting      bool
	ctrlCPressed  bool
	completions   []int // indices into slashCommands
	completionIdx int   // selected completion
	showComplete  bool  // whether to show completion menu
}

// ctrlCResetMsg is sent to reset the ctrl+c state after a timeout
type ctrlCResetMsg struct{}

func (m inputModel) Init() tea.Cmd {
	return textarea.Blink
}

// getCompletions returns matching command indices for current input
func (m *inputModel) getCompletions() []int {
	text := m.textarea.Value()
	if !strings.HasPrefix(text, "/") || strings.Contains(text, "\n") {
		return nil
	}

	var matches []int
	for i, cmd := range slashCommands {
		if strings.HasPrefix(cmd.name, text) {
			matches = append(matches, i)
		}
	}
	return matches
}

func (m inputModel) Update(msg tea.Msg) (tea.Model, tea.Cmd) {
	switch msg := msg.(type) {
	case ctrlCResetMsg:
		m.ctrlCPressed = false
		return m, nil

	case tea.KeyMsg:
		// Any key other than Ctrl+C resets the exit state
		if msg.Type != tea.KeyCtrlC {
			m.ctrlCPressed = false
		}

		// Handle completion navigation
		if m.showComplete && len(m.completions) > 0 {
			switch msg.Type {
			case tea.KeyTab, tea.KeyDown:
				m.completionIdx = (m.completionIdx + 1) % len(m.completions)
				return m, nil
			case tea.KeyShiftTab, tea.KeyUp:
				m.completionIdx = (m.completionIdx - 1 + len(m.completions)) % len(m.completions)
				return m, nil
			case tea.KeyEnter:
				// Select completion
				cmd := slashCommands[m.completions[m.completionIdx]]
				m.textarea.Reset()
				m.textarea.InsertString(cmd.name)
				m.showComplete = false
				m.completions = nil
				return m, nil
			case tea.KeyEsc:
				m.showComplete = false
				m.completions = nil
				return m, nil
			}
		}

		switch msg.Type {
		case tea.KeyCtrlC:
			if m.ctrlCPressed {
				// Second Ctrl+C - actually quit
				m.quitting = true
				return m, tea.Quit
			}
			// First Ctrl+C - show warning
			m.ctrlCPressed = true
			return m, tea.Tick(time.Second, func(t time.Time) tea.Msg {
				return ctrlCResetMsg{}
			})
		case tea.KeyCtrlD:
			m.quitting = true
			return m, tea.Quit
		case tea.KeyCtrlJ:
			// Ctrl+J inserts newline (ASCII line feed)
			m.textarea.InsertString("\n")
			m.updateHeight()
			return m, nil
		case tea.KeyEnter:
			// Enter submits
			m.value = strings.TrimSpace(m.textarea.Value())
			if m.value == "" {
				return m, nil
			}
			if m.value == "/quit" || m.value == "/exit" {
				m.quitting = true
			}
			return m, tea.Quit
		case tea.KeyTab:
			// Tab triggers completion if typing a command
			completions := m.getCompletions()
			if len(completions) > 0 {
				m.completions = completions
				m.completionIdx = 0
				m.showComplete = true
				return m, nil
			}
		}
	}

	var cmd tea.Cmd
	m.textarea, cmd = m.textarea.Update(msg)
	m.updateHeight()

	// Update completions as user types
	if strings.HasPrefix(m.textarea.Value(), "/") {
		m.completions = m.getCompletions()
		m.showComplete = len(m.completions) > 0
		if m.completionIdx >= len(m.completions) {
			m.completionIdx = 0
		}
	} else {
		m.showComplete = false
		m.completions = nil
	}

	return m, cmd
}

// updateHeight adjusts textarea height based on content
func (m *inputModel) updateHeight() {
	content := m.textarea.Value()
	lines := strings.Count(content, "\n") + 1
	// Clamp between 1 and 10 lines
	if lines < 1 {
		lines = 1
	}
	if lines > 10 {
		lines = 10
	}
	m.textarea.SetHeight(lines)
}

func (m inputModel) View() string {
	view := m.textarea.View()

	// Show completion menu
	if m.showComplete && len(m.completions) > 0 {
		view += "\n"
		for i, idx := range m.completions {
			cmd := slashCommands[idx]
			if i == m.completionIdx {
				view += PromptStyle.Render("→ "+cmd.name) + " " + SystemStyle.Render(cmd.desc) + "\n"
			} else {
				view += SystemStyle.Render("  "+cmd.name+" "+cmd.desc) + "\n"
			}
		}
	}

	if m.ctrlCPressed {
		view += "\n" + SystemStyle.Render("Press Ctrl+C again to exit")
	}
	return view
}
