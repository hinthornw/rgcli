package ui

import (
	"fmt"

	tea "github.com/charmbracelet/bubbletea"
	"github.com/charmbracelet/lipgloss"

	"github.com/wfh/lsc/internal/api"
)

var (
	selectedStyle = lipgloss.NewStyle().
			Foreground(lipgloss.Color("14")).
			Bold(true)

	unselectedStyle = lipgloss.NewStyle().
			Foreground(lipgloss.Color("8"))
)

// PickerModel is the Bubbletea model for thread selection
type PickerModel struct {
	threads  []api.Thread
	cursor   int
	selected *api.Thread
	quitting bool
}

// NewPickerModel creates a new picker model
func NewPickerModel(threads []api.Thread) PickerModel {
	return PickerModel{
		threads: threads,
		cursor:  0,
	}
}

// Init initializes the picker
func (m PickerModel) Init() tea.Cmd {
	return nil
}

// Update handles messages
func (m PickerModel) Update(msg tea.Msg) (tea.Model, tea.Cmd) {
	switch msg := msg.(type) {
	case tea.KeyMsg:
		switch msg.Type {
		case tea.KeyCtrlC, tea.KeyEsc:
			m.quitting = true
			return m, tea.Quit

		case tea.KeyUp, tea.KeyShiftTab:
			if m.cursor > 0 {
				m.cursor--
			}

		case tea.KeyDown, tea.KeyTab:
			if m.cursor < len(m.threads)-1 {
				m.cursor++
			}

		case tea.KeyEnter:
			if len(m.threads) > 0 {
				m.selected = &m.threads[m.cursor]
			}
			return m, tea.Quit
		}
	}

	return m, nil
}

// View renders the picker
func (m PickerModel) View() string {
	if m.quitting {
		return ""
	}

	s := "Select a thread to resume:\n\n"

	for i, thread := range m.threads {
		preview := GetThreadPreview(thread)
		threadID := thread.ThreadID
		if len(threadID) > 8 {
			threadID = threadID[:8]
		}

		line := fmt.Sprintf("%s - %s", threadID, preview)

		if i == m.cursor {
			s += selectedStyle.Render("> " + line)
		} else {
			s += unselectedStyle.Render("  " + line)
		}
		s += "\n"
	}

	s += "\n" + unselectedStyle.Render("(↑/↓ to move, enter to select, esc to cancel)")

	return s
}

// Selected returns the selected thread
func (m PickerModel) Selected() *api.Thread {
	return m.selected
}

// IsQuitting returns true if the user cancelled
func (m PickerModel) IsQuitting() bool {
	return m.quitting
}

// PickThread runs the thread picker and returns the selected thread
func PickThread(threads []api.Thread) (*api.Thread, error) {
	if len(threads) == 0 {
		return nil, fmt.Errorf("no threads found")
	}

	model := NewPickerModel(threads)
	p := tea.NewProgram(model)

	finalModel, err := p.Run()
	if err != nil {
		return nil, err
	}

	m := finalModel.(PickerModel)
	if m.IsQuitting() {
		return nil, nil
	}

	return m.Selected(), nil
}
