package main

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"strings"
	"time"

	opensandbox "github.com/alibaba/OpenSandbox/sdks/sandbox/go"
)

const (
	execdPort       = 44772
	scriptFixture   = "from pathlib import Path\nprint('m5-sandbox-ok')\nPath('/workspace/result.txt').write_text('m5-artifact-ok', encoding='utf-8')"
	expectedStdout  = "m5-sandbox-ok"
	artifactFixture = "m5-artifact-ok"
	inputPath       = "/workspace/main.py"
	outputPath      = "/workspace/result.txt"
)

type result struct {
	SandboxID        string  `json:"sandboxId"`
	LifecycleState   string  `json:"lifecycleState"`
	Endpoint         string  `json:"endpoint"`
	Stdout           string  `json:"stdout"`
	ExitCode         int     `json:"exitCode"`
	DownloadSHA256   string  `json:"downloadSha256"`
	CPUCount         float64 `json:"cpuCount"`
	MemoryTotalMiB   float64 `json:"memoryTotalMiB"`
	UploadMatched    bool    `json:"uploadMatched"`
	CommandMatched   bool    `json:"commandMatched"`
	DownloadMatched  bool    `json:"downloadMatched"`
	ListMatched      bool    `json:"listMatched"`
	NetworkMatched   bool    `json:"networkPolicyMatched"`
	InterruptMatched bool    `json:"interruptMatched"`
	Terminated       bool    `json:"terminated"`
}

func main() {
	value, err := run()
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	if err := json.NewEncoder(os.Stdout).Encode(value); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}

func run() (result, error) {
	endpoint := strings.TrimRight(os.Getenv("AGENTX_OPENSANDBOX_ENDPOINT"), "/")
	apiKey := os.Getenv("AGENTX_OPENSANDBOX_API_KEY")
	image := os.Getenv("AGENTX_OPENSANDBOX_IMAGE")
	if endpoint == "" || apiKey == "" || image == "" {
		return result{}, fmt.Errorf("AGENTX_OPENSANDBOX_ENDPOINT, AGENTX_OPENSANDBOX_API_KEY and AGENTX_OPENSANDBOX_IMAGE are required")
	}

	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Minute)
	defer cancel()
	ttl := 180
	config := opensandbox.ConnectionConfig{
		Domain:           endpoint,
		APIKey:           apiKey,
		UseServerProxy:   true,
		RequestTimeout:   30 * time.Second,
		DisableMetrics:   true,
		EndpointCacheTTL: time.Minute,
	}
	sandbox, err := opensandbox.CreateSandbox(ctx, config, opensandbox.SandboxCreateOptions{
		Image:          image,
		Entrypoint:     []string{"tail", "-f", "/dev/null"},
		ResourceLimits: opensandbox.ResourceLimits{"cpu": "500m", "memory": "512Mi", "pids": "128", "ephemeral-storage": "1Gi"},
		TimeoutSeconds: &ttl,
		SecureAccess:   false,
		Metadata:       map[string]string{"agentx-contract": "go-oracle"},
		NetworkPolicy: &opensandbox.NetworkPolicy{
			DefaultAction: "deny",
			Egress:        []opensandbox.NetworkRule{{Action: "allow", Target: "example.com"}},
		},
	})
	if err != nil {
		return result{}, err
	}
	killed := false
	defer func() {
		if !killed {
			cleanup, cleanupCancel := context.WithTimeout(context.Background(), 30*time.Second)
			defer cleanupCancel()
			_ = sandbox.Kill(cleanup)
		}
	}()

	info, err := sandbox.GetInfo(ctx)
	if err != nil {
		return result{}, err
	}
	endpointInfo, err := sandbox.GetEndpoint(ctx, execdPort)
	if err != nil {
		return result{}, err
	}
	if err := sandbox.UploadFile(ctx, strings.NewReader(scriptFixture), opensandbox.UploadFileOptions{
		FileName: "main.py",
		Metadata: opensandbox.FileMetadata{Path: inputPath, Mode: 600},
	}); err != nil {
		return result{}, err
	}

	execution, err := sandbox.RunCommandWithOpts(ctx, opensandbox.RunCommandRequest{
		Command: "python3 " + inputPath,
		Timeout: 30_000,
	}, nil)
	if err != nil {
		return result{}, err
	}
	if execution.ExitCode == nil {
		return result{}, fmt.Errorf("official Go SDK command result did not include an exit code")
	}
	if *execution.ExitCode != 0 {
		stderr := make([]string, 0, len(execution.Stderr))
		for _, message := range execution.Stderr {
			stderr = append(stderr, message.Text)
		}
		return result{}, fmt.Errorf("official Go SDK command failed with exit code %d: %s", *execution.ExitCode, strings.Join(stderr, "\n"))
	}
	manager := opensandbox.NewSandboxManager(config)
	listed, err := manager.ListSandboxInfos(ctx, opensandbox.ListOptions{
		Metadata: map[string]string{"agentx-contract": "go-oracle"},
		PageSize: 100,
	})
	if err != nil {
		return result{}, err
	}
	listMatched := false
	for _, candidate := range listed.Items {
		if candidate.ID == sandbox.ID() {
			listMatched = true
			break
		}
	}
	if !listMatched {
		return result{}, fmt.Errorf("official Go SDK list did not find the labeled sandbox")
	}

	allowed, err := sandbox.RunCommandWithOpts(ctx, opensandbox.RunCommandRequest{
		Command: "python3 -c \"import urllib.request; print(urllib.request.urlopen('https://example.com', timeout=10).status)\"",
		Timeout: 20_000,
	}, nil)
	if err != nil {
		return result{}, err
	}
	denied, err := sandbox.RunCommandWithOpts(ctx, opensandbox.RunCommandRequest{
		Command: "python3 -c \"import urllib.request; urllib.request.urlopen('https://iana.org', timeout=5)\"",
		Timeout: 15_000,
	}, nil)
	if err != nil {
		return result{}, err
	}
	networkMatched := allowed.ExitCode != nil && *allowed.ExitCode == 0 &&
		strings.Contains(allowed.Text(), "200") && denied.ExitCode != nil && *denied.ExitCode != 0
	if !networkMatched {
		return result{}, fmt.Errorf("official Go SDK network policy fixture did not enforce deny with allowlist")
	}

	background, err := sandbox.RunCommandWithOpts(ctx, opensandbox.RunCommandRequest{
		Command:    "sleep 30",
		Background: true,
		Timeout:    30_000,
	}, nil)
	if err != nil {
		return result{}, err
	}
	if background.ID == "" {
		return result{}, fmt.Errorf("official Go SDK background command did not return an ID")
	}
	execdHeaders := make(map[string]string, len(endpointInfo.Headers)+1)
	for key, value := range endpointInfo.Headers {
		execdHeaders[key] = value
	}
	if _, ok := execdHeaders["X-EXECD-ACCESS-TOKEN"]; !ok {
		execdHeaders["X-EXECD-ACCESS-TOKEN"] = apiKey
	}
	execdURL := config.RewriteEndpointURL(endpointInfo.Endpoint)
	if !strings.HasPrefix(execdURL, "http://") && !strings.HasPrefix(execdURL, "https://") {
		execdURL = config.GetProtocol() + "://" + execdURL
	}
	execd := opensandbox.NewExecdClient(execdURL, "", opensandbox.WithHeaders(execdHeaders))
	if err := execd.InterruptCommand(ctx, background.ID); err != nil {
		return result{}, err
	}

	download, err := sandbox.DownloadFile(ctx, outputPath, "")
	if err != nil {
		return result{}, err
	}
	downloaded, readErr := io.ReadAll(io.LimitReader(download, 1<<20))
	closeErr := download.Close()
	if readErr != nil {
		return result{}, readErr
	}
	if closeErr != nil {
		return result{}, closeErr
	}
	metrics, err := sandbox.GetMetrics(ctx)
	if err != nil {
		return result{}, err
	}

	wantHash := sha256.Sum256([]byte(artifactFixture))
	gotHash := sha256.Sum256(downloaded)
	stdout := execution.Text()
	value := result{
		SandboxID:        sandbox.ID(),
		LifecycleState:   string(info.Status.State),
		Endpoint:         endpointInfo.Endpoint,
		Stdout:           stdout,
		ExitCode:         *execution.ExitCode,
		DownloadSHA256:   hex.EncodeToString(gotHash[:]),
		CPUCount:         metrics.CPUCount,
		MemoryTotalMiB:   metrics.MemTotalMB,
		UploadMatched:    hex.EncodeToString(gotHash[:]) == hex.EncodeToString(wantHash[:]),
		CommandMatched:   stdout == expectedStdout && *execution.ExitCode == 0,
		DownloadMatched:  string(downloaded) == artifactFixture,
		ListMatched:      listMatched,
		NetworkMatched:   networkMatched,
		InterruptMatched: true,
	}
	if !value.UploadMatched || !value.CommandMatched || !value.DownloadMatched ||
		!value.ListMatched || !value.NetworkMatched || !value.InterruptMatched {
		return result{}, fmt.Errorf("official Go SDK fixture result did not match the Agentx contract")
	}
	if metrics.CPUCount <= 0 || metrics.MemTotalMB <= 0 {
		return result{}, fmt.Errorf("official Go SDK returned invalid metrics")
	}

	if err := sandbox.Kill(ctx); err != nil {
		return result{}, err
	}
	killed = true
	value.Terminated = true
	return value, nil
}
