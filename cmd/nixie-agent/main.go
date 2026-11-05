package main

import (
	"context"
	"fmt"
	"log"
	"os"
	"os/signal"
	"syscall"
	"time"
)

func main() {
	if err := run(); err != nil {
		log.Fatal(err)
	}
}

func run() error {
	if len(os.Args) != 1 {
		return fmt.Errorf("nixie-agent takes no arguments; it runs from the custom installer service")
	}

	params, err := getAgentConfig()
	if err != nil {
		return fmt.Errorf("read installer parameters: %w", err)
	}

	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()

	ctx, cancel := context.WithTimeout(ctx, 45*time.Minute)
	defer cancel()

	if err := install(ctx, *params); err != nil {
		return fmt.Errorf("install system: %w", err)
	}

	return nil
}
