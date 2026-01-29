package ui

import "github.com/charmbracelet/lipgloss"

var (
	// Colors
	UserColor      = lipgloss.Color("12")  // Blue
	AssistantColor = lipgloss.Color("10")  // Green
	SystemColor    = lipgloss.Color("8")   // Gray
	ErrorColor     = lipgloss.Color("9")   // Red
	PromptColor    = lipgloss.Color("14")  // Cyan
	LogoAccent     = lipgloss.Color("209") // Soft coral/salmon
	LogoBody       = lipgloss.Color("37")  // Muted teal

	// Styles
	UserStyle = lipgloss.NewStyle().
			Foreground(UserColor).
			Bold(true)

	AssistantStyle = lipgloss.NewStyle().
			Foreground(AssistantColor)

	SystemStyle = lipgloss.NewStyle().
			Foreground(SystemColor).
			Italic(true)

	ErrorStyle = lipgloss.NewStyle().
			Foreground(ErrorColor).
			Bold(true)

	PromptStyle = lipgloss.NewStyle().
			Foreground(PromptColor).
			Bold(true)

	SpinnerStyle = lipgloss.NewStyle().
			Foreground(PromptColor)

	// Logo styles
	logoAccent = lipgloss.NewStyle().Foreground(LogoAccent)
	logoBody   = lipgloss.NewStyle().Foreground(LogoBody)
	logoTitle  = lipgloss.NewStyle().Foreground(PromptColor).Bold(true)
	logoInfo   = lipgloss.NewStyle().Foreground(SystemColor)

	// Role labels
	UserLabel      = UserStyle.Render("You: ")
	AssistantLabel = AssistantStyle.Render("Assistant: ")
)

// Logo returns the colored parrot logo with version info
func Logo(version, endpoint, configPath string) string {
	a := logoAccent.Render
	b := logoBody.Render

	title := logoTitle.Render("lsc") + " " + logoInfo.Render(version)
	info1 := logoInfo.Render(endpoint)
	info2 := logoInfo.Render(configPath)

	// Combine parrot with text on the right
	lines := []string{
		"   " + a("▄█▀▀█▄"),
		"  " + a("▄██") + b("▄░▄") + a("█") + "    " + title,
		"  " + b("███████") + "    " + info1,
		"  " + b("▀█░░░█") + "     " + info2,
		"   " + b("█▀ █▀"),
	}

	result := ""
	for _, line := range lines {
		result += line + "\n"
	}
	return result
}
