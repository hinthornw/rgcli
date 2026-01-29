package config

import (
	"os"
	"path/filepath"

	"gopkg.in/yaml.v3"
)

type Config struct {
	Endpoint      string            `yaml:"endpoint"`
	ApiKey        string            `yaml:"api_key"`
	AssistantID   string            `yaml:"assistant_id"`
	CustomHeaders map[string]string `yaml:"custom_headers,omitempty"`
}

// ConfigDir returns the path to the config directory (~/.lsc)
func ConfigDir() (string, error) {
	home, err := os.UserHomeDir()
	if err != nil {
		return "", err
	}
	return filepath.Join(home, ".lsc"), nil
}

// ConfigPath returns the path to the config file (~/.lsc/config.yaml)
func ConfigPath() (string, error) {
	dir, err := ConfigDir()
	if err != nil {
		return "", err
	}
	return filepath.Join(dir, "config.yaml"), nil
}

// Exists checks if the config file exists
func Exists() bool {
	path, err := ConfigPath()
	if err != nil {
		return false
	}
	_, err = os.Stat(path)
	return err == nil
}

// Load reads the config from disk
func Load() (*Config, error) {
	path, err := ConfigPath()
	if err != nil {
		return nil, err
	}

	data, err := os.ReadFile(path)
	if err != nil {
		return nil, err
	}

	var cfg Config
	if err := yaml.Unmarshal(data, &cfg); err != nil {
		return nil, err
	}

	return &cfg, nil
}

// Save writes the config to disk
func Save(cfg *Config) error {
	dir, err := ConfigDir()
	if err != nil {
		return err
	}

	// Ensure directory exists
	if err := os.MkdirAll(dir, 0700); err != nil {
		return err
	}

	path, err := ConfigPath()
	if err != nil {
		return err
	}

	data, err := yaml.Marshal(cfg)
	if err != nil {
		return err
	}

	return os.WriteFile(path, data, 0600)
}

// GetHeaders returns all headers needed for API requests
func (c *Config) GetHeaders() map[string]string {
	headers := map[string]string{
		"Content-Type": "application/json",
	}
	if c.ApiKey != "" {
		headers["X-Api-Key"] = c.ApiKey
	}
	for k, v := range c.CustomHeaders {
		headers[k] = v
	}
	return headers
}
