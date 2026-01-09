# Releasing lsc

## One-time Setup

### 1. Create the Homebrew Tap Repository

Create a new GitHub repository called `homebrew-tap`:

```bash
# On GitHub, create: wfh/homebrew-tap
# Or via gh CLI:
gh repo create homebrew-tap --public --description "Homebrew formulas"
```

Initialize it with a Formula directory:

```bash
git clone https://github.com/wfh/homebrew-tap.git
cd homebrew-tap
mkdir Formula
echo "# Homebrew Tap\n\nInstall formulas:\n\n\`\`\`bash\nbrew tap wfh/tap\nbrew install lsc\n\`\`\`" > README.md
git add .
git commit -m "Initial commit"
git push
```

### 2. Create a Personal Access Token

GoReleaser needs a token to push to the homebrew-tap repo:

1. Go to https://github.com/settings/tokens
2. Generate new token (classic)
3. Select scopes: `repo` (full control)
4. Copy the token

### 3. Add the Token as a Repository Secret

1. Go to your `lsc` repository settings
2. Secrets and variables → Actions
3. New repository secret
4. Name: `HOMEBREW_TAP_GITHUB_TOKEN`
5. Value: (paste your token)

## Making a Release

### 1. Update Version (optional)

The version comes from the git tag, but update any hardcoded references if needed.

### 2. Commit and Tag

```bash
git add .
git commit -m "Prepare v0.1.0 release"
git tag v0.1.0
git push origin main --tags
```

### 3. Watch the Release

The GitHub Action will:
1. Build binaries for darwin/linux (amd64/arm64)
2. Create a GitHub Release with the binaries
3. Push the Homebrew formula to wfh/homebrew-tap

Check progress at: https://github.com/wfh/lsc/actions

### 4. Verify Installation

```bash
brew tap wfh/tap
brew install lsc
lsc --help
```

## Local Testing

Test the release locally without publishing:

```bash
goreleaser release --snapshot --clean
```

This creates binaries in `dist/` without pushing anywhere.
