package main

import (
	"context"
	"flag"
	"fmt"
	"os"

	"github.com/charmbracelet/huh"

	"github.com/wfh/lsc/internal/api"
	"github.com/wfh/lsc/internal/config"
	"github.com/wfh/lsc/internal/ui"
)

func main() {
	resume := flag.Bool("resume", false, "Resume an existing thread")
	showVersion := flag.Bool("version", false, "Show version")
	flag.Parse()

	if *showVersion {
		fmt.Printf("lsc %s\n", version)
		return
	}

	if err := run(*resume); err != nil {
		fmt.Fprintln(os.Stderr, ui.PrintError(err.Error()))
		os.Exit(1)
	}
}

// version is set by goreleaser ldflags
var version = "dev"

func run(resume bool) error {
	// Check if config exists
	if !config.Exists() {
		fmt.Println("Welcome to lsc! Let's configure your connection.")
		fmt.Println()
		if err := runConfigure(); err != nil {
			return err
		}
		fmt.Println()
	}

	// Load config
	cfg, err := config.Load()
	if err != nil {
		return fmt.Errorf("failed to load config: %w", err)
	}

	// Show splash
	configPath, _ := config.ConfigPath()
	fmt.Print(ui.Logo(version, cfg.Endpoint, configPath))
	fmt.Println()

	// Create API client
	client := api.NewClient(cfg)

	// Handle resume flow
	var threadID string
	var history []api.Message

	if resume {
		thread, messages, err := handleResume(client)
		if err != nil {
			return err
		}
		if thread == nil {
			// User cancelled
			return nil
		}
		threadID = thread.ThreadID
		history = messages
	} else {
		// Create new thread
		ctx := context.Background()
		thread, err := client.CreateThread(ctx)
		if err != nil {
			return fmt.Errorf("failed to create thread: %w", err)
		}
		threadID = thread.ThreadID
	}

	// Run chat loop
	for {
		err := ui.RunChatLoop(client, cfg.AssistantID, threadID, history)
		if err != nil && err.Error() == "CONFIGURE" {
			// User wants to reconfigure
			if err := runConfigure(); err != nil {
				return err
			}
			// Reload config
			cfg, err = config.Load()
			if err != nil {
				return fmt.Errorf("failed to reload config: %w", err)
			}
			client = api.NewClient(cfg)
			history = nil // Clear history since we might have new settings
			continue
		}
		return err
	}
}

func runConfigure() error {
	var endpoint, apiKey, assistantID string
	var authType string
	customHeaders := make(map[string]string)

	// Load existing config if available
	if config.Exists() {
		cfg, err := config.Load()
		if err == nil {
			endpoint = cfg.Endpoint
			apiKey = cfg.ApiKey
			assistantID = cfg.AssistantID
			customHeaders = cfg.CustomHeaders
			// Determine auth type from existing config
			if apiKey != "" {
				authType = "apikey"
			} else if len(customHeaders) > 0 {
				authType = "headers"
			} else {
				authType = "none"
			}
		}
	}

	// Step 1: Endpoint URL
	if endpoint == "" {
		endpoint = "https://chat-langchain-993a2fee078256ab879993a971197820.us.langgraph.app"
	}
	err := huh.NewInput().
		Title("Endpoint URL").
		Description("Press Enter to accept default").
		Value(&endpoint).
		Run()
	if err != nil {
		return err
	}
	if endpoint == "" {
		return fmt.Errorf("endpoint URL is required")
	}

	// Step 2: Auth type selection
	err = huh.NewSelect[string]().
		Title("Authentication").
		Description("How should we authenticate with this deployment?").
		Options(
			huh.NewOption("None (public endpoint)", "none"),
			huh.NewOption("LangSmith API Key", "apikey"),
			huh.NewOption("Custom Headers", "headers"),
		).
		Value(&authType).
		Run()
	if err != nil {
		return err
	}

	// Step 3: Auth details based on selection
	switch authType {
	case "apikey":
		err = huh.NewInput().
			Title("LangSmith API Key").
			Description("Your API key (starts with lsv2_)").
			Placeholder("lsv2_sk_...").
			EchoMode(huh.EchoModePassword).
			Value(&apiKey).
			Run()
		if err != nil {
			return err
		}
		customHeaders = nil // Clear any custom headers

	case "headers":
		apiKey = "" // Clear API key
		if customHeaders == nil {
			customHeaders = make(map[string]string)
		}

		// Prompt for custom headers
		addMore := true
		for addMore {
			var headerName, headerValue string

			err = huh.NewInput().
				Title("Header Name").
				Placeholder("Authorization").
				Value(&headerName).
				Run()
			if err != nil {
				return err
			}

			if headerName == "" {
				break
			}

			err = huh.NewInput().
				Title("Header Value").
				Placeholder("Bearer xxx...").
				EchoMode(huh.EchoModePassword).
				Value(&headerValue).
				Run()
			if err != nil {
				return err
			}

			customHeaders[headerName] = headerValue

			err = huh.NewConfirm().
				Title("Add another header?").
				Value(&addMore).
				Run()
			if err != nil {
				return err
			}
		}

	case "none":
		apiKey = ""
		customHeaders = nil
	}

	// Step 4: Assistant ID
	if assistantID == "" {
		assistantID = "docs_agent"
	}
	err = huh.NewInput().
		Title("Assistant ID").
		Description("Press Enter to accept default").
		Value(&assistantID).
		Run()
	if err != nil {
		return err
	}
	if assistantID == "" {
		assistantID = "docs_agent"
	}

	// Save config
	cfg := &config.Config{
		Endpoint:      endpoint,
		ApiKey:        apiKey,
		AssistantID:   assistantID,
		CustomHeaders: customHeaders,
	}

	if err := config.Save(cfg); err != nil {
		return fmt.Errorf("failed to save config: %w", err)
	}

	path, _ := config.ConfigPath()
	fmt.Printf("\nConfiguration saved to %s\n", path)

	return nil
}

func handleResume(client *api.Client) (*api.Thread, []api.Message, error) {
	ctx := context.Background()

	// Search for threads
	fmt.Println("Searching for threads...")
	threads, err := client.SearchThreads(ctx, 20)
	if err != nil {
		return nil, nil, fmt.Errorf("failed to search threads: %w", err)
	}

	if len(threads) == 0 {
		fmt.Println("No existing threads found. Starting a new conversation.")
		thread, err := client.CreateThread(ctx)
		if err != nil {
			return nil, nil, fmt.Errorf("failed to create thread: %w", err)
		}
		return thread, nil, nil
	}

	// Let user pick a thread
	fmt.Print("\033[F\033[K") // Clear "Searching..." line
	selected, err := ui.PickThread(threads)
	if err != nil {
		return nil, nil, err
	}
	if selected == nil {
		return nil, nil, nil
	}

	// Get thread state to load history
	state, err := client.GetThreadState(ctx, selected.ThreadID)
	if err != nil {
		// Non-fatal - just continue without history
		return selected, nil, nil
	}

	messages := api.GetMessages(state.Values)
	return selected, messages, nil
}
